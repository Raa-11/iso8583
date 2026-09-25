//! ISO 8583 TCP client to test the server and measure throughput.
//!
//! Sends `count` 0200 requests (2-byte little-endian header) pipelined over a single connection:
//! one thread writes while the main thread reads responses.
//!
//! Standard library only, so it can run on Linux or Windows (server remains on Linux/WSL).
//!
//! Run (server must already be running):
//!   cargo run --release --bin client -- 127.0.0.1:5000 100000

use std::io::{self, BufReader, BufWriter, Read, Write};
use std::net::TcpStream;
use std::time::Instant;

const REQ: &[u8] = b"02007020000000810000164111111111111111000000000000010000123456TERM0001011hello world";

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let addr = args.next().unwrap_or_else(|| "127.0.0.1:5000".into());
    let count: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(100_000);

    let stream = TcpStream::connect(&addr)?;
    stream.set_nodelay(true)?;

    let mut frame = (REQ.len() as u16).to_le_bytes().to_vec();
    frame.extend_from_slice(REQ);
    println!("request  : {:02X?} + {}", &frame[..2], String::from_utf8_lossy(REQ));

    let start = Instant::now();
    let writer = {
        let mut w = BufWriter::with_capacity(64 * 1024, stream.try_clone()?);
        std::thread::spawn(move || -> io::Result<()> {
            for _ in 0..count {
                w.write_all(&frame)?;
            }
            w.flush()
        })
    };

    let mut rd = BufReader::with_capacity(64 * 1024, stream);
    let mut body = Vec::with_capacity(4096);
    for i in 0..count {
        let mut hdr = [0u8; 2];
        rd.read_exact(&mut hdr)?;
        body.resize(u16::from_le_bytes(hdr) as usize, 0);
        rd.read_exact(&mut body)?;
        if i == 0 {
            println!("response : {:02X?} + {}", hdr, String::from_utf8_lossy(&body));
        }
    }
    let elapsed = start.elapsed();
    writer.join().expect("writer thread panicked")?;

    println!(
        "{} messages in {:.2?} → {:.0} msg/s (round-trip, 1 connection)",
        count,
        elapsed,
        count as f64 / elapsed.as_secs_f64()
    );
    Ok(())
}
