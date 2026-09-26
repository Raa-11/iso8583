# iso_8583_rs

[![Crates.io](https://img.shields.io/crates/v/iso_8583_rs.svg)](https://crates.io/crates/iso_8583_rs)
[![Docs.rs](https://img.shields.io/docsrs/iso_8583_rs)](https://docs.rs/iso_8583_rs)
[![License](https://img.shields.io/crates/l/iso_8583_rs.svg)](#license)

**A fast ISO 8583 parser and builder for Rust. Zero-copy, zero-allocation, and configured by a YAML spec.**

ISO 8583 is the message format behind card payments: ATM withdrawals, POS purchases, network echo tests, and switch-to-switch traffic. This crate reads and writes those messages in about **100–150 nanoseconds** without touching the heap, and lets you describe each host's message layout in a plain YAML file instead of code.

```text
                          this crate (0.2)     published 0.1.1     speed-up
Parse, full validation        155 ns              29 500 ns          ~190×
Parse, lazy                   103 ns              29 500 ns          ~285×
Pack                          102 ns               3 560 ns          ~35×
```
<sub>Single core, AMD Ryzen 7 7840HS, 11-field 0200 message. Details and method in [Benchmarks](#benchmarks).</sub>

## Contents

- [Why iso_8583_rs?](#why-iso_8583_rs)
- [Features](#features)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Usage guide](#usage-guide)
- [Benchmarks](#benchmarks)
- [How it works](#how-it-works)
- [Supported format and limitations](#supported-format-and-limitations)
- [Roadmap](#roadmap)
- [Example: io_uring TCP server](#example-io_uring-tcp-server)
- [Development](#development)
- [Security](#security)
- [License](#license)

## Why iso_8583_rs?

**Every host speaks its own dialect.** ISO 8583 only defines a framework: which fields exist, how long they are, and how they are encoded differs between banks, switches, and card schemes. Libraries that hard-code one layout force you to fork or patch code for each new connection. Here the layout is a YAML file, so supporting another host means writing another spec, not another parser. The included [`spec1987.yml`](spec1987.yml) covers the full ISO 8583:1987 field set, and specs written for [mofax/iso8583](https://github.com/mofax/iso8583) load as-is.

**Parsing sits on the hot path of every transaction.** A payment switch parses and builds a message for every request and response. Parsers that allocate a `String` per field and copy the message around add latency spikes and memory pressure exactly where you want neither. This crate borrows the input buffer, stores only offsets, and allocates nothing while parsing or packing. A test that counts every allocation enforces it.

**Input comes from the network, so it is hostile by default.** Truncated frames, garbage bitmaps, oversized length prefixes, and unknown fields are routine in production. Every failure here is a small typed `Error` you can match on; the parser never panics on malformed input, and every truncation of a valid message is tested.

**A note on expectations.** In a real switch, network round-trips, HSM calls, and database queries cost microseconds to milliseconds. Parsing at ~150 ns will not be your bottleneck; the point is that it stops being a cost at all, stays predictable, and does not allocate.

## Features

| | |
|---|---|
| **Zero-copy** | `Message<'a>` borrows the input buffer. Field values are `&[u8]` slices, never `String`s. |
| **Zero allocation** | Parsing and packing never touch the heap (verified by `tests/zero_alloc.rs`). |
| **YAML-driven spec** | Per-host field layouts; compiled once into a flat table that fits in L1 cache. |
| **Validated at the boundary** | Length and content type (`n`, `a`, `an`, `ans`) are checked, 8 bytes per step. |
| **Strict or lazy parsing** | Validate everything in one pass, or only the structure and validate fields as you read them. |
| **Typed errors** | A small `Copy` enum, no allocation on the error path, and no panics on bad input. |
| **Fast numbers** | `get_u64` reads amounts, STANs, and dates 8 digits at a time. |
| **Small** | No `unsafe`, two dependencies (`serde`, `serde-saphyr`), both pure Rust. |

## Installation

```toml
[dependencies]
iso_8583_rs = "0.2"
```

## Quick start

```rust
use iso_8583_rs::{Builder, CompiledSpec, Message};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load and compile the spec once at startup.
    let spec = CompiledSpec::from_file("spec1987.yml")?;

    // Build: values are borrowed and validated once on `set`.
    let mut req = Builder::new(&spec, b"0200")?;
    req.set(2, b"4111111111111111")?
        .set(3, b"000000")?
        .set(4, b"000000010000")?
        .set(11, b"123456")?
        .set(41, b"TERM0001")?;

    let mut buf = Vec::new();
    req.pack_into(&mut buf); // reuse `buf` across messages → no allocation after the first

    // Parse: zero-copy, validates length and content type.
    let msg = Message::parse(&spec, &buf)?;
    assert_eq!(&msg.mti, b"0200");
    assert_eq!(msg.get(2), Some(&b"4111111111111111"[..]));
    assert_eq!(msg.get_u64(4), Some(10_000));

    for (field, value) in msg.fields() {
        println!("{field:>3}: {}", String::from_utf8_lossy(value));
    }
    Ok(())
}
```

## Usage guide

Every Rust example in this guide is compiled and run by `cargo test`, so it always matches the current API.

1. [Spec file](#1-spec-file)
2. [Load and share the spec](#2-load-and-share-the-spec)
3. [Build and pack a message](#3-build-and-pack-a-message)
4. [Parse and read a message](#4-parse-and-read-a-message)
5. [Lazy parsing](#5-lazy-parsing)
6. [Modify an incoming message](#6-modify-an-incoming-message)
7. [Reply to a request](#7-reply-to-a-request)
8. [Handle errors](#8-handle-errors)
9. [Send and receive over TCP](#9-send-and-receive-over-tcp)
10. [Standalone helpers](#10-standalone-helpers)
11. [Legacy API](#11-legacy-api)

### 1. Spec file

The spec is a YAML map from field number to definition. [`spec1987.yml`](spec1987.yml) is a full ISO 8583:1987 spec you can start from.

```yaml
2:
  ContentType: n        # n | a | an | ans  (anything else is not content-checked)
  MaxLen: 19
  MinLen: 12            # optional, default 0
  LenType: llvar        # fixed | llvar | lllvar | llllvar
  Label: Primary Account Number   # optional
41:
  ContentType: ans
  MaxLen: 8             # for fixed fields MaxLen is the exact length
  LenType: fixed
  Label: Card acceptor terminal identification
```

| `ContentType` | Allowed characters |
|---|---|
| `n` | digits |
| `a` | letters and space |
| `an` | letters, digits, and space |
| `ans` | printable ASCII (`0x20`–`0x7E`) |
| anything else (`b`, `z`, …) | not checked (length is still checked) |

| `LenType` | Length prefix |
|---|---|
| `fixed` | none; the length is `MaxLen` |
| `llvar` | 2 digits (max 99) |
| `lllvar` | 3 digits (max 999) |
| `llllvar` | 4 digits (max 9999) |

Entries `0` (MTI), `1` and `65` (bitmap indicators) are ignored, so the [mofax/iso8583](https://github.com/mofax/iso8583/blob/main/spec1987.yml) spec file loads as-is. Invalid definitions are rejected when the spec is loaded, for example `MinLen > MaxLen`, or an `llvar` with `MaxLen > 99`.

### 2. Load and share the spec

Compile the spec **once** at startup. Every parse and build borrows it, so share one instance across threads:

```rust
use std::sync::OnceLock;
use iso_8583_rs::CompiledSpec;

static SPEC: OnceLock<CompiledSpec> = OnceLock::new();

/// Global spec, loaded on first use. `&'static` works across threads with no locking.
fn spec() -> &'static CompiledSpec {
    SPEC.get_or_init(|| CompiledSpec::from_file("spec1987.yml").expect("valid spec file"))
}

fn main() {
    let handles: Vec<_> = (0..4)
        .map(|_| std::thread::spawn(|| spec().fields[2].max_len))
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap(), 19);
    }
}
```

You can also define a spec in code, for example in tests:

```rust
use std::collections::HashMap;
use iso_8583_rs::specfile::{FieldDescription, Spec};
use iso_8583_rs::CompiledSpec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let field = |content: &str, len_type: &str, max_len, min_len| FieldDescription {
        content_type: content.into(),
        len_type: len_type.into(),
        max_len,
        min_len,
        label: String::new(),
    };
    let spec = CompiledSpec::compile(&Spec {
        fields: HashMap::from([
            (2, field("n", "llvar", 19, 12)),
            (3, field("n", "fixed", 6, 0)),
        ]),
    })?;
    assert!(spec.fields[3].present);
    Ok(())
}
```

### 3. Build and pack a message

```rust
use iso_8583_rs::{Builder, CompiledSpec};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let spec = CompiledSpec::from_file("spec1987.yml")?;

    let mut req = Builder::new(&spec, b"0200")?;
    req.set(2, b"4111111111111111")?     // llvar: length prefix is added for you
        .set(3, b"000000")?
        .set(4, b"000000010000")?        // amount 100.00 in minor units
        .set(11, b"123456")?
        .set(41, b"TERM0001")?
        .set(102, b"ACC-1")?;            // field > 64: secondary bitmap is added for you

    // Setting a field again replaces its value.
    req.set(11, b"123457")?;

    // Fixed-length fields must be exactly MaxLen: pad short values yourself.
    let terminal = format!("{:<8}", "ATM1"); // "ATM1    "
    req.set(41, terminal.as_bytes())?;

    // Option A: into a Vec you reuse across messages (no allocation once it is big enough).
    let mut out = Vec::with_capacity(512);
    req.pack_into(&mut out);

    // Option B: into a stack buffer; the exact size is known up front.
    let mut stack = [0u8; 1024];
    let n = req.pack_to_slice(&mut stack)?;
    assert_eq!(n, req.packed_len());
    assert_eq!(&stack[..n], &out[..]);
    Ok(())
}
```

`set` validates the value against the spec (length and content type) immediately, so a packed message always matches the spec. The bitmap is written in upper-case hex.

### 4. Parse and read a message

```rust
use iso_8583_rs::{CompiledSpec, Message};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let spec = CompiledSpec::from_file("spec1987.yml")?;
    let raw = b"02007020000000810000164111111111111111000000000000010000123456TERM0001011hello world";

    let msg = Message::parse(&spec, raw)?;       // structure + content validation

    assert_eq!(&msg.mti, b"0200");
    assert!(msg.has(41));
    assert!(!msg.has(39));

    // Raw bytes (borrowed from `raw`, no copy)
    assert_eq!(msg.get(41), Some(&b"TERM0001"[..]));
    // As &str
    assert_eq!(msg.get_str(48), Some("hello world"));
    // As a number (amount, STAN, …)
    assert_eq!(msg.get_u64(4), Some(10_000));
    assert_eq!(msg.get_u64(11), Some(123_456));
    // Absent field
    assert_eq!(msg.get(39), None);

    // Every present field, in field-number order
    let present: Vec<usize> = msg.fields().map(|(n, _)| n).collect();
    assert_eq!(present, [2, 3, 4, 11, 41, 48]);

    // Secondary bitmap present? (field 1 bit)
    assert!(msg.bitmap >> 127 == 0);
    Ok(())
}
```

`Message` borrows the input buffer, so keep the buffer alive while you use the message. To keep a field after the buffer is gone, copy it: `msg.get(41).map(<[u8]>::to_vec)`.

### 5. Lazy parsing

When a service only needs a few fields (a router reading MTI, field 3 and field 41, for example), skip content validation for the rest:

```rust
use iso_8583_rs::{CompiledSpec, Error, Message};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let spec = CompiledSpec::from_file("spec1987.yml")?;
    // Field 2 (PAN) contains a letter, which is invalid for an `n` field.
    let raw = b"0200702000000081000016X111111111111111000000000000010000123456TERM0001011hello world";

    // Strict parse rejects the whole message.
    assert_eq!(Message::parse(&spec, raw).unwrap_err(), Error::Invalid(2));

    // Lazy parse checks only the structure (bitmap, lengths, buffer bounds)…
    let msg = Message::parse_lazy(&spec, raw)?;
    // …and validates only the fields you read.
    assert_eq!(msg.get_validated(&spec, 41)?, Some(&b"TERM0001"[..]));
    assert_eq!(msg.get_validated(&spec, 2), Err(Error::Invalid(2)));

    // Or validate everything later.
    assert_eq!(msg.validate(&spec), Err(Error::Invalid(2)));
    Ok(())
}
```

The structure is always checked in both modes, so lazy parsing is still safe for untrusted input.

### 6. Modify an incoming message

A parsed `Message` is read-only (it borrows the input). To change a field, copy the fields into a `Builder`, change what you need, and pack again. Copying only borrows slices; nothing is allocated except the output buffer.

```rust
use iso_8583_rs::{Builder, CompiledSpec, Error, Message};

/// Replace field 41 (terminal ID), drop field 48, keep everything else.
fn rewrite(spec: &CompiledSpec, incoming: &[u8], terminal: &[u8], out: &mut Vec<u8>) -> Result<(), Error> {
    let msg = Message::parse(spec, incoming)?;
    let mut b = Builder::new(spec, &msg.mti)?;
    for (n, v) in msg.fields().filter(|&(n, _)| n != 48) {  // skip a field to remove it
        b.set(n, v)?;
    }
    b.set(41, terminal)?;                                    // replace (or add) a field
    b.pack_into(out);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let spec = CompiledSpec::from_file("spec1987.yml")?;
    let incoming = b"02007020000000810000164111111111111111000000000000010000123456TERM0001011hello world";

    let mut out = Vec::new();
    rewrite(&spec, incoming, b"ATM-JKT1", &mut out)?;

    let msg = Message::parse(&spec, &out)?;
    assert_eq!(msg.get(41), Some(&b"ATM-JKT1"[..]));
    assert_eq!(msg.get(48), None);
    assert_eq!(msg.get(2), Some(&b"4111111111111111"[..])); // untouched
    Ok(())
}
```

### 7. Reply to a request

A response usually echoes the request fields, changes the MTI (`0200` → `0210`), and adds field 39 (response code):

```rust
use iso_8583_rs::{Builder, CompiledSpec, Error, Message};

fn reply(spec: &CompiledSpec, request: &[u8], out: &mut Vec<u8>) -> Result<(), Error> {
    let req = Message::parse(spec, request)?;

    // Third MTI digit: even = request/advice, +1 = response (0200 → 0210, 0800 → 0810).
    let mut mti = req.mti;
    if (mti[2] - b'0') % 2 != 0 {
        return Err(Error::BadMti); // already a response
    }
    mti[2] += 1;

    let mut resp = Builder::new(spec, &mti)?;
    for (n, v) in req.fields() {
        resp.set(n, v)?;
    }
    let code: &[u8] = match req.get_u64(4) {
        Some(amount) if amount > 1_000_000 => b"51", // insufficient funds
        _ => b"00",                                  // approved
    };
    resp.set(39, code)?;
    resp.pack_into(out);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let spec = CompiledSpec::from_file("spec1987.yml")?;
    let request = b"02007020000000810000164111111111111111000000000000010000123456TERM0001011hello world";

    let mut out = Vec::new();
    reply(&spec, request, &mut out)?;

    let resp = Message::parse(&spec, &out)?;
    assert_eq!(&resp.mti, b"0210");
    assert_eq!(resp.get_str(39), Some("00"));
    Ok(())
}
```

### 8. Handle errors

Every failure is a small `Copy` enum, so you can match on it and it never allocates:

```rust
use iso_8583_rs::{CompiledSpec, Error, Message};

fn describe(spec: &CompiledSpec, raw: &[u8]) -> String {
    match Message::parse(spec, raw) {
        Ok(msg) => format!("ok, {} fields", msg.fields().count()),
        Err(Error::TooShort) => "truncated message".into(),
        Err(Error::BadMti) => "MTI must be 4 digits".into(),
        Err(Error::BadHex) => "bitmap is not hex".into(),
        Err(Error::UnknownField(n)) => format!("field {n} is not in the spec"),
        Err(Error::BadLength(n)) => format!("field {n} has a wrong length"),
        Err(Error::Invalid(n)) => format!("field {n} has invalid characters"),
        Err(Error::TrailingBytes) => "extra bytes after the last field (spec mismatch?)".into(),
        Err(e) => format!("rejected: {e}"), // TooLong, TertiaryUnsupported, BadSpec
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let spec = CompiledSpec::from_file("spec1987.yml")?;
    assert_eq!(describe(&spec, b"0200"), "truncated message");
    assert_eq!(describe(&spec, b"02X0"), "MTI must be 4 digits");
    assert_eq!(describe(&spec, b"0200ZZ00000000000000"), "bitmap is not hex");
    Ok(())
}
```

`Error` implements `std::error::Error` and `Display`, so it also works with `?` into `Box<dyn Error>`, `anyhow`, and similar.

### 9. Send and receive over TCP

ISO 8583 over TCP usually puts a length header in front of every message. This example uses a 2-byte binary little-endian header; use `to_be_bytes` / `from_be_bytes` if your host expects big-endian.

```rust,no_run
use std::io::{self, Read, Write};
use std::net::TcpStream;
use iso_8583_rs::{Builder, CompiledSpec, Message};

/// Write one frame: 2-byte little-endian length + message.
fn write_frame(w: &mut impl Write, msg: &[u8]) -> io::Result<()> {
    w.write_all(&(msg.len() as u16).to_le_bytes())?;
    w.write_all(msg)
}

/// Read one frame into `buf` (reused across messages).
fn read_frame(r: &mut impl Read, buf: &mut Vec<u8>) -> io::Result<()> {
    let mut len = [0u8; 2];
    r.read_exact(&mut len)?;
    buf.resize(u16::from_le_bytes(len) as usize, 0);
    r.read_exact(buf)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let spec = CompiledSpec::from_file("spec1987.yml")?;
    let mut stream = TcpStream::connect("127.0.0.1:5000")?;
    stream.set_nodelay(true)?;

    // 0800 network management (echo test)
    let mut echo = Builder::new(&spec, b"0800")?;
    echo.set(7, b"0925120000")?.set(11, b"000001")?.set(70, b"301")?;

    let mut out = Vec::new();
    echo.pack_into(&mut out);
    write_frame(&mut stream, &out)?;

    let mut buf = Vec::new();
    read_frame(&mut stream, &mut buf)?;
    let resp = Message::parse(&spec, &buf)?;
    println!("{} → response code {:?}", String::from_utf8_lossy(&resp.mti), resp.get_str(39));
    Ok(())
}
```

For a full server (io_uring, async requests, graceful shutdown) see [Example: io_uring TCP server](#example-io_uring-tcp-server).

### 10. Standalone helpers

The digit parser and the content-type validators are public, for use on your own data:

```rust
use iso_8583_rs::charset::{all_alpha, all_alphanumeric, all_digits, all_printable};
use iso_8583_rs::parse_digits_u64;

fn main() {
    // ASCII digits → u64 (1..=19 digits), None if empty, too long, or not digits.
    assert_eq!(parse_digits_u64(b"000000010000"), Some(10_000));
    assert_eq!(parse_digits_u64(b"12a"), None);

    assert!(all_digits(b"0123456789"));      // n
    assert!(all_alpha(b"ABC def"));          // a
    assert!(all_alphanumeric(b"ATM 01"));    // an
    assert!(all_printable(b"ATM-01/JKT"));   // ans
}
```

### 11. Legacy API

The original `IsoStruct` API from 0.1.x is still available for existing code. It is now built on the new parser and no longer panics on malformed input, but it allocates and compiles the spec on every parse, so prefer `Message` / `Builder` for new code.

```rust
use iso_8583_rs::iso8583::new_iso_struct;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut iso = new_iso_struct("spec1987.yml", false)?; // true = 128-bit bitmap
    iso.add_mti("0200")?;
    iso.add_field(2, "4111111111111111")?;
    iso.add_field(41, "TERM0001")?;

    let raw = iso.to_string()?;          // bitmap in lower-case hex
    let parsed = iso.parse(&raw)?;
    assert_eq!(parsed.mti.mti, "0200");
    assert_eq!(parsed.elements.get_elements()[&41], "TERM0001");
    Ok(())
}
```

## Benchmarks

### Setup

| | |
|---|---|
| CPU | AMD Ryzen 7 7840HS (laptop), single core |
| OS / Rust | Windows 11, Rust 1.97 (stable), `--release` with `lto = "fat"`, `codegen-units = 1` |
| Tool | [criterion](https://crates.io/crates/criterion) 0.5: 2 s warm-up, 5 s measurement, `black_box` on inputs |
| Message | one 0200 with 11 fields (2, 3, 4, 7, 11, 12, 13, 37, 41, 42, 49), 120 bytes |
| Spec | the full ISO 8583:1987 spec in [`spec1987.yml`](spec1987.yml), 126 fields |
| Baseline | the published **0.1.1** from crates.io, pulled in as a renamed dev-dependency |

### Results

Lower time is better. Throughput is `1 / time` on one core.

| Operation | 0.2 time | 0.2 throughput | 0.1.1 time | 0.1.1 throughput | Speed-up |
|---|---|---|---|---|---|
| Parse, full validation (`Message::parse`) | **155 ns** | ~6.5 M msg/s | 29 500 ns | ~34 K msg/s | **~190×** |
| Parse, structure only (`Message::parse_lazy`) | **103 ns** | ~9.7 M msg/s | 29 500 ns | ~34 K msg/s | **~285×** |
| Pack an existing message (`pack_into`) | **102 ns** | ~9.8 M msg/s | 3 560 ns | ~281 K msg/s | **~35×** |
| Build + pack from scratch (`Builder`) | **367 ns** | ~2.7 M msg/s | 3 560 ns | ~281 K msg/s | **~10×** |
| Heap allocations per parse or pack | **0** | | 1 spec clone + 1 `String` per field | | |

### How to read these numbers

- **Full validation vs lazy.** Strict parsing also checks minimum lengths and every field's characters; the extra ~50 ns is that validation. Lazy parsing checks only structure and leaves content checks to `get_validated`.
- **Why 0.1.1 is so much slower.** Each 0.1.1 `parse` clones the entire spec (126 fields, 3 `String`s each) and copies the rest of the message once per field, so its cost grows with both spec size and message size. This crate compiles the spec once and borrows the input.
- **Same work on both sides.** The benchmark asserts that both versions produce the same message (ignoring hex case) before timing anything.
- **One accommodation for 0.1.1.** It requires `MinLen` on every field, while `spec1987.yml` (like the original Go spec) omits it on many. The benchmark feeds 0.1.1 a temporary copy with `MinLen: 0` added. `0` is also the new default, so this changes nothing about the work being measured.
- **Laptop noise.** Expect roughly ±10 % between runs (turbo, thermals, background load). For steadier numbers, plug in the charger, pick the high-performance power mode, and close other programs.
- **Not an end-to-end number.** These measure parsing and packing only. Network, TLS, HSM, and database time are not included.

### Reproduce

```bash
cargo bench --bench parse
```

The benchmark lives in [`benches/parse.rs`](benches/parse.rs). Results with plots are written to `target/criterion/`.

### Server throughput

For the io_uring example server (WSL2, one connection): about **460 000 round-trips/s** when the business logic is instant, and about **143 000 round-trips/s** when every request waits 5 ms for a simulated HSM or database call, because requests wait concurrently. A blocking, one-request-at-a-time server manages about 200/s at that latency. See [Example: io_uring TCP server](#example-io_uring-tcp-server).

## How it works

- **Bitmap as a `u128`.** Fields 1–128 map to the bits of one integer (bit 127 = field 1). Finding the next present field is `leading_zeros` (one instruction), and "how many fields come before field *n*" is `count_ones`, which makes `get(n)` O(1).
- **Compiled spec.** The YAML is turned into a `[FieldDef; 129]` table once. A field lookup is an array access, and the whole table (~1 KB) stays in L1 cache. Nothing is hashed or compared as text while parsing.
- **Offsets, not copies.** Parsing records `(offset, length)` per field and never copies data. `get` hands back a slice of your original buffer.
- **One pass.** Strict parsing validates each field in the same loop, while its bytes are still hot in cache. A const generic removes the validation code entirely from the lazy path.
- **SWAR validation.** ISO 8583 fields are short (3–20 bytes), too short for SIMD to pay off, so digit and printable-ASCII checks test 8 bytes at a time inside a single `u64`. `get_u64` uses the same idea to convert 8 digits per step.
- **Cold error path.** Error construction is kept out of the hot loop, and `Error` is a 2-byte `Copy` enum, so a flood of malformed messages does not allocate either.

The source is split into small modules, one concern each (`message`, `builder`, `spec`, `specfile`, `bitmap`, `length`, `charset`, `numeric`, `error`), and every public item is documented. See [docs.rs](https://docs.rs/iso_8583_rs).

## Supported format and limitations

| | |
|---|---|
| Encoding | ASCII |
| MTI | 4 ASCII digits |
| Bitmap | hex ASCII, primary + secondary (fields 1–128) |
| Length types | `fixed`, `llvar`, `lllvar`, `llllvar` |
| Max message size | 65 535 bytes |

Not supported (yet):

- BCD and EBCDIC encodings, and binary bitmaps. The bitmap must be hex ASCII.
- A tertiary bitmap (field 65 set, fields 129–192); such messages are rejected.
- Named field constants, response-code and processing-code helpers, Luhn check, and PAN masking. Fields are addressed by number, for example `msg.get(2)`.
- Removing a field from a `Builder`. Skip the field while copying instead (see [Modify an incoming message](#6-modify-an-incoming-message)).
- `no_std`. The crate requires `std`.

## Roadmap

Ideas that are not implemented yet. Open an issue if one of them matters to you.

- BCD / EBCDIC field encodings and binary bitmaps.
- Tertiary bitmap (fields 129–192).
- A lighter `Builder` that avoids the ~2 KB stack array it initialises (about 70 ns per `Builder::new`).
- An optional compile-time spec (generated by `build.rs`) for hosts whose layout never changes.
- Server example: idle-connection timeout, and a 4-byte length header for messages over 64 KB.

## Example: io_uring TCP server

[`examples/tcp-uring`](examples/tcp-uring) is a production-shaped ISO 8583 host server. It runs on Linux (including WSL2) and shows the library in a real network service:

- io_uring via [monoio](https://github.com/bytedance/monoio), thread-per-core with `SO_REUSEPORT`, with automatic epoll fallback
- 2-byte **little-endian** length header framing and persistent connections
- every request handled in its own async task; responses are written as soon as they are ready (out of order, matched by STAN/RRN)
- per-connection backpressure (at most 1024 in-flight requests)
- responses batched into a single write
- graceful shutdown on Ctrl+C / SIGTERM: stop accepting, finish in-flight requests, flush, close (30 s grace; a second Ctrl+C forces exit)

```bash
cd examples/tcp-uring
cargo run --release --bin iso8583-tcp-uring -- 127.0.0.1:5000 ../../spec1987.yml 5   # 5 ms simulated HSM/DB latency
cargo run --release --bin client -- 127.0.0.1:5000 100000
```

## Development

```bash
cargo test                  # unit, integration, zero-allocation, and every README example
cargo bench --bench parse   # 0.2 vs the published 0.1.1
cargo clippy --all-targets
```

## Security

This library handles message **structure** only. In production you still need TLS on the transport, PIN encryption and key management through an HSM, and MAC generation and verification. Never log full PANs.

## License

Licensed under either of [MIT](https://opensource.org/licenses/MIT) or [Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0), at your option.
