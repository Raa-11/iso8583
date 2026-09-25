//! ISO 8583 TCP server with io_uring (monoio, thread-per-core). Linux only.
//!
//! Framing: 2-byte little-endian binary length header + ISO message (max 65535 bytes per message).
//!
//! Design:
//! - Thread-per-core: one monoio runtime per core, each with its own listener on the same port
//!   (SO_REUSEPORT) so the kernel distributes connections. No locks/atomics on the hot path.
//! - Each request is processed in its own async task; responses are sent as soon as they are ready
//!   (may be out of order, matched by the client via STAN/RRN like host-to-host ISO 8583).
//! - Backpressure: at most `MAX_IN_FLIGHT` requests processed per connection; beyond that the server
//!   stops reading from the socket until some complete.
//! - Per-connection writer batches multiple responses into a single write (fewer syscalls).
//! - Graceful shutdown (Ctrl+C / SIGTERM): stops accepting connections and stops reading new
//!   requests; already-received requests are processed and their responses sent, then connections
//!   are closed. Wait timeout `SHUTDOWN_GRACE`; a second Ctrl+C forces an immediate exit.
//!
//! Run (from this folder, on Linux/WSL):
//!   cargo run --release --bin iso8583-tcp-uring -- 127.0.0.1:5000 ../../spec1987.yml [latency_ms]
//!   cargo run --release --bin client -- 127.0.0.1:5000 100000
//! `latency_ms` simulates an async call to HSM/DB per request (default 0).

use std::io;
use std::net::SocketAddr;
use std::rc::Rc;
use std::time::Duration;

use bytes::{Buf, Bytes, BytesMut};
use iso_8583_rs::{Builder, CompiledSpec, Error, Message};
use local_sync::mpsc::bounded::{self, Tx};
use local_sync::semaphore::Semaphore;
use monoio::buf::IoBufMut;
use monoio::io::{AsyncReadRent, AsyncWriteRent, AsyncWriteRentExt, Splitable};
use monoio::net::{ListenerOpts, TcpListener, TcpStream};
use monoio::{FusionDriver, RuntimeBuilder};
use tokio_util::sync::CancellationToken;

const HEADER: usize = 2;
const READ_CHUNK: usize = 64 * 1024;
const WRITE_BATCH: usize = 256 * 1024;
const MAX_IN_FLIGHT: usize = 1024;
const SHUTDOWN_GRACE: Duration = Duration::from_secs(30);

struct Config {
    spec: CompiledSpec,
    /// Simulated HSM/DB latency per request.
    latency: Duration,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let addr: SocketAddr = args.next().as_deref().unwrap_or("127.0.0.1:5000").parse()?;
    let spec_path = args.next().unwrap_or_else(|| "../../spec1987.yml".into());
    let latency_ms: u64 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(0);

    // Read once, shared across all threads as &'static.
    let cfg: &'static Config = Box::leak(Box::new(Config {
        spec: CompiledSpec::from_file(&spec_path)?,
        latency: Duration::from_millis(latency_ms),
    }));

    let token = CancellationToken::new();
    {
        let token = token.clone();
        ctrlc::set_handler(move || {
            if token.is_cancelled() {
                eprintln!("forced exit");
                std::process::exit(1);
            }
            eprintln!("shutdown: no longer accepting connections, finishing in-flight requests...");
            token.cancel();
        })?;
    }

    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let driver = if monoio::utils::detect_uring() { "io_uring" } else { "epoll (io_uring tidak tersedia)" };
    println!(
        "ISO 8583 server di {} ({}, {} thread, header 2 byte LE, latency {} ms)",
        addr, driver, threads, latency_ms
    );
    run(addr, cfg, threads, token)?;
    println!("server stopped cleanly");
    Ok(())
}

/// Run `threads` workers and wait until all of them finish (after `token` is cancelled).
fn run(addr: SocketAddr, cfg: &'static Config, threads: usize, token: CancellationToken) -> io::Result<()> {
    let workers = (0..threads)
        .map(|id| {
            let token = token.clone();
            std::thread::Builder::new().name(format!("worker-{}", id)).spawn(move || {
                let mut rt = RuntimeBuilder::<FusionDriver>::new().enable_timer().build()?;
                rt.block_on(worker(id, addr, cfg, token))
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    for w in workers {
        w.join().expect("worker panic")?;
    }
    Ok(())
}

async fn worker(id: usize, addr: SocketAddr, cfg: &'static Config, token: CancellationToken) -> io::Result<()> {
    let listener = TcpListener::bind_with_config(addr, &ListenerOpts::new().reuse_port(true))?;
    // Each connection holds an `alive` clone; recv() returns None when all connections have finished.
    let (alive, mut all_closed) = bounded::channel::<()>(1);

    loop {
        let accepted = monoio::select! {
            _ = token.cancelled() => None,
            r = listener.accept() => Some(r),
        };
        match accepted {
            None => break,
            Some(Ok((stream, peer))) => {
                monoio::spawn(connection(stream, peer, cfg, token.clone(), alive.clone()));
            }
            Some(Err(e)) => {
                // e.g. EMFILE: do not busy-loop
                eprintln!("[w{}] accept: {}", id, e);
                monoio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }

    drop(listener);
    drop(alive);
    if monoio::time::timeout(SHUTDOWN_GRACE, all_closed.recv()).await.is_err() {
        eprintln!("[w{}] grace period expired, closing remaining connections", id);
    }
    Ok(())
}

async fn connection(
    stream: TcpStream,
    peer: SocketAddr,
    cfg: &'static Config,
    token: CancellationToken,
    _alive: Tx<()>,
) {
    let _ = stream.set_nodelay(true);
    let (mut rd, wr) = stream.into_split();

    // Responses from request tasks → single writer. Writer finishes when all Tx are dropped.
    let (resp_tx, resp_rx) = bounded::channel::<Vec<u8>>(MAX_IN_FLIGHT);
    let writer = monoio::spawn(write_loop(wr, resp_rx, peer));
    let in_flight = Rc::new(Semaphore::new(MAX_IN_FLIGHT));

    let mut buf = BytesMut::with_capacity(2 * READ_CHUNK);
    let mut received = 0u64;
    loop {
        while let Some(frame) = next_frame(&mut buf) {
            received += 1;
            let Ok(permit) = in_flight.clone().acquire_owned().await else { break };
            let tx = resp_tx.clone();
            monoio::spawn(async move {
                if let Some(resp) = handle(cfg, &frame).await {
                    let _ = tx.send(resp).await; // Err = writer has stopped (connection disconnected)
                }
                drop(permit);
            });
        }

        // Ensure at least READ_CHUNK capacity; one frame (max 65537 bytes) can always be accumulated.
        buf.reserve(READ_CHUNK);
        let len = buf.len();
        // monoio writes from buffer start, so read into a slice after existing data.
        let read = rd.read(buf.slice_mut(len..));
        let r = monoio::select! {
            _ = token.cancelled() => None,
            r = read => Some(r),
        };
        let Some((res, slice)) = r else { break }; // shutdown: stop reading new requests
        buf = slice.into_inner();
        match res {
            Ok(0) => break, // client closed connection
            Ok(_) => {}
            Err(e) => {
                eprintln!("{}: read error: {}", peer, e);
                break;
            }
        }
    }

    // Wait for all in-flight requests to complete and their responses to be sent.
    drop(resp_tx);
    let sent = writer.await;
    if received != sent {
        eprintln!("{}: {} requests received, {} responses sent", peer, received, sent);
    }
}

/// Extract one full frame from the front of the buffer; `None` if not enough data yet.
fn next_frame(buf: &mut BytesMut) -> Option<Bytes> {
    let len = u16::from_le_bytes(buf.get(..HEADER)?.try_into().unwrap()) as usize;
    if buf.len() < HEADER + len {
        return None;
    }
    buf.advance(HEADER);
    Some(buf.split_to(len).freeze()) // zero-copy: frame shares memory with the read buffer
}

/// Send incoming responses; batch queued responses into a single write. Returns count sent.
async fn write_loop(
    mut wr: impl AsyncWriteRent,
    mut rx: bounded::Rx<Vec<u8>>,
    peer: SocketAddr,
) -> u64 {
    let mut out = Vec::with_capacity(WRITE_BATCH);
    let mut sent = 0;
    while let Some(first) = rx.recv().await {
        out.clear();
        out.extend_from_slice(&first);
        let mut batch = 1;
        while out.len() < WRITE_BATCH {
            match rx.try_recv() {
                Ok(more) => {
                    out.extend_from_slice(&more);
                    batch += 1;
                }
                Err(_) => break,
            }
        }
        let (res, b) = wr.write_all(out).await;
        out = b;
        if let Err(e) = res {
            eprintln!("{}: write error: {}", peer, e);
            break;
        }
        sent += batch;
    }
    let _ = wr.shutdown().await;
    sent
}

/// Process a single request. `None` = message rejected (logged, connection keeps running).
async fn handle(cfg: &Config, frame: &[u8]) -> Option<Vec<u8>> {
    let result = async {
        let msg = Message::parse(&cfg.spec, frame)?;
        let code = authorize(cfg, &msg).await;
        build_response(&cfg.spec, &msg, code)
    };
    match result.await {
        Ok(resp) => Some(resp),
        Err(e) => {
            eprintln!("message rejected ({} bytes): {}", frame.len(), e);
            None
        }
    }
}

/// Example business logic. Location for async calls to HSM/DB/core banking.
async fn authorize(cfg: &Config, msg: &Message<'_>) -> &'static [u8] {
    if !cfg.latency.is_zero() {
        monoio::time::sleep(cfg.latency).await;
    }
    match msg.get_u64(4) {
        Some(amount) if amount > 1_000_000 => b"51",
        _ => b"00",
    }
}

/// Response = all request fields + field 39, preceded by a 2-byte LE header.
fn build_response(spec: &CompiledSpec, msg: &Message<'_>, code: &[u8]) -> Result<Vec<u8>, Error> {
    let mut mti = msg.mti;
    if !(mti[2] - b'0').is_multiple_of(2) {
        return Err(Error::BadMti); // already a response
    }
    mti[2] += 1;

    let mut resp = Builder::new(spec, &mti)?;
    for (n, v) in msg.fields() {
        resp.set(n, v)?;
    }
    resp.set(39, code)?;

    let len = resp.packed_len();
    let len16 = u16::try_from(len).map_err(|_| Error::TooLong)?;
    let mut out = vec![0u8; HEADER + len];
    out[..HEADER].copy_from_slice(&len16.to_le_bytes());
    resp.pack_to_slice(&mut out[HEADER..])?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::time::Instant;

    const REQ: &[u8] = b"02007020000000810000164111111111111111000000000000010000123456TERM0001011hello world";

    fn frame(body: &[u8]) -> Vec<u8> {
        let mut f = (body.len() as u16).to_le_bytes().to_vec();
        f.extend_from_slice(body);
        f
    }

    #[test]
    fn next_frame_handles_partial_and_multiple() {
        let mut all = frame(b"abc");
        all.extend(frame(b""));
        all.extend(frame(b"hello"));
        let mut buf = BytesMut::new();
        let mut got = Vec::new();
        // feed bytes one by one: header & body split across all positions
        for &b in &all {
            buf.extend_from_slice(&[b]);
            while let Some(f) = next_frame(&mut buf) {
                got.push(f);
            }
        }
        assert_eq!(got, [&b"abc"[..], b"", b"hello"]);
        assert!(buf.is_empty());
        // little-endian header: 0x0102 = 258
        let mut buf = BytesMut::from(&[0x02, 0x01][..]);
        buf.extend_from_slice(&[0u8; 257]);
        assert!(next_frame(&mut buf).is_none());
        buf.extend_from_slice(&[0]);
        assert_eq!(next_frame(&mut buf).unwrap().len(), 258);
    }

    #[test]
    fn response_has_le_header_and_code() {
        let spec = CompiledSpec::from_file("../../spec1987.yml").unwrap();
        let msg = Message::parse(&spec, REQ).unwrap();
        let out = build_response(&spec, &msg, b"00").unwrap();
        let len = u16::from_le_bytes([out[0], out[1]]) as usize;
        assert_eq!(len, out.len() - HEADER);
        let m = Message::parse(&spec, &out[HEADER..]).unwrap();
        assert_eq!(&m.mti, b"0210");
        assert_eq!(m.get(39), Some(&b"00"[..]));
    }

    /// Shutdown while requests are in flight: all accepted requests are still answered,
    /// then connections are closed and workers terminate.
    #[test]
    fn graceful_shutdown_finishes_in_flight() {
        let cfg: &'static Config = Box::leak(Box::new(Config {
            spec: CompiledSpec::from_file("../../spec1987.yml").unwrap(),
            latency: Duration::from_millis(300),
        }));
        // find an unused port
        let addr = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let token = CancellationToken::new();
        let server = {
            let token = token.clone();
            std::thread::spawn(move || run(addr, cfg, 2, token))
        };

        let mut c = loop {
            match std::net::TcpStream::connect(addr) {
                Ok(c) => break c,
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        let n = 50;
        let mut frames = Vec::new();
        for _ in 0..n {
            frames.extend(frame(REQ));
        }
        c.write_all(&frames).unwrap();

        // request is "waiting for HSM" (300 ms) when shutdown starts
        std::thread::sleep(Duration::from_millis(100));
        let started = Instant::now();
        token.cancel();

        let mut responses = 0;
        loop {
            let mut hdr = [0u8; 2];
            if c.read_exact(&mut hdr).is_err() {
                break; // server closes connection after all responses are sent
            }
            let mut body = vec![0u8; u16::from_le_bytes(hdr) as usize];
            c.read_exact(&mut body).unwrap();
            let m = Message::parse(&cfg.spec, &body).unwrap();
            assert_eq!(m.get(39), Some(&b"00"[..]));
            responses += 1;
        }
        assert_eq!(responses, n, "all in-flight requests must be answered");
        server.join().unwrap().unwrap();
        // requests are processed in parallel: total ~300 ms, not 50 × 300 ms
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    }
}
