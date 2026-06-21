//! RESP2 (REdis Serialization Protocol) — enough to speak to real `redis-cli`
//! and `redis-benchmark`/`memtier_benchmark`. Requests are arrays of bulk
//! strings (with an inline-command fallback); replies are simple strings,
//! errors, integers, and bulk strings. Pure parse/encode, unit-tested.

use std::io::{self, BufRead, Write};

/// A reply the server can render to the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Simple(String),
    Error(String),
    Int(i64),
    Bulk(Option<Vec<u8>>),
    Array(Vec<Reply>),
}

impl Reply {
    pub fn ok() -> Self {
        Reply::Simple("OK".into())
    }
    pub fn pong() -> Self {
        Reply::Simple("PONG".into())
    }

    /// Serialize to RESP2 wire bytes.
    pub fn write_to<W: Write>(&self, w: &mut W) -> io::Result<()> {
        match self {
            Reply::Simple(s) => write!(w, "+{s}\r\n"),
            Reply::Error(s) => write!(w, "-{s}\r\n"),
            Reply::Int(n) => write!(w, ":{n}\r\n"),
            Reply::Bulk(None) => w.write_all(b"$-1\r\n"),
            Reply::Bulk(Some(b)) => {
                write!(w, "${}\r\n", b.len())?;
                w.write_all(b)?;
                w.write_all(b"\r\n")
            }
            Reply::Array(items) => {
                write!(w, "*{}\r\n", items.len())?;
                for it in items {
                    it.write_to(w)?;
                }
                Ok(())
            }
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::new();
        self.write_to(&mut v).expect("vec write");
        v
    }
}

/// Read one command — a vector of byte arguments — from `r`. Returns `Ok(None)`
/// at clean EOF. Supports the RESP array form (`*N $len ...`) and an inline
/// fallback (whitespace-split), exactly as Redis does.
pub fn read_command<R: BufRead>(r: &mut R) -> io::Result<Option<Vec<Vec<u8>>>> {
    let mut line = Vec::new();
    if read_line(r, &mut line)? == 0 {
        return Ok(None);
    }
    if line.is_empty() {
        return Ok(Some(Vec::new()));
    }
    if line[0] == b'*' {
        let n: i64 = parse_int(&line[1..])?;
        if n <= 0 {
            return Ok(Some(Vec::new()));
        }
        let mut args = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let mut hdr = Vec::new();
            if read_line(r, &mut hdr)? == 0 {
                return Err(unexpected_eof());
            }
            if hdr.first() != Some(&b'$') {
                return Err(protocol("expected bulk string"));
            }
            let len: i64 = parse_int(&hdr[1..])?;
            if len < 0 {
                args.push(Vec::new());
                continue;
            }
            let mut buf = vec![0u8; len as usize + 2]; // +CRLF
            r.read_exact(&mut buf)?;
            buf.truncate(len as usize);
            args.push(buf);
        }
        Ok(Some(args))
    } else {
        // Inline command: split on ASCII whitespace.
        let args = line
            .split(|b| b.is_ascii_whitespace())
            .filter(|s| !s.is_empty())
            .map(<[u8]>::to_vec)
            .collect();
        Ok(Some(args))
    }
}

/// Read up to and including `\n`, returning the line without trailing CRLF.
/// Returns the number of bytes consumed (0 at EOF).
fn read_line<R: BufRead>(r: &mut R, out: &mut Vec<u8>) -> io::Result<usize> {
    let mut raw = Vec::new();
    let n = r.read_until(b'\n', &mut raw)?;
    if n == 0 {
        return Ok(0);
    }
    while matches!(raw.last(), Some(b'\n' | b'\r')) {
        raw.pop();
    }
    *out = raw;
    Ok(n)
}

fn parse_int(b: &[u8]) -> io::Result<i64> {
    std::str::from_utf8(b)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| protocol("invalid integer"))
}

fn protocol(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("RESP protocol error: {msg}"))
}
fn unexpected_eof() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "RESP: unexpected EOF")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parses_resp_array_command() {
        let mut c = Cursor::new(b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$3\r\nbar\r\n".to_vec());
        let cmd = read_command(&mut c).unwrap().unwrap();
        assert_eq!(cmd, vec![b"SET".to_vec(), b"foo".to_vec(), b"bar".to_vec()]);
    }

    #[test]
    fn parses_binary_safe_values() {
        // value contains CRLF and NUL — must be length-delimited, not line-split.
        let mut c = Cursor::new(b"*3\r\n$3\r\nSET\r\n$1\r\nk\r\n$4\r\n\x00\r\n\xff\r\n".to_vec());
        let cmd = read_command(&mut c).unwrap().unwrap();
        assert_eq!(cmd[2], vec![0x00, b'\r', b'\n', 0xff]);
    }

    #[test]
    fn inline_command_fallback() {
        let mut c = Cursor::new(b"PING\r\n".to_vec());
        assert_eq!(read_command(&mut c).unwrap().unwrap(), vec![b"PING".to_vec()]);
    }

    #[test]
    fn eof_returns_none() {
        let mut c = Cursor::new(Vec::new());
        assert!(read_command(&mut c).unwrap().is_none());
    }

    #[test]
    fn reply_encodings() {
        assert_eq!(Reply::ok().to_bytes(), b"+OK\r\n");
        assert_eq!(Reply::Int(42).to_bytes(), b":42\r\n");
        assert_eq!(Reply::Bulk(None).to_bytes(), b"$-1\r\n");
        assert_eq!(Reply::Bulk(Some(b"hi".to_vec())).to_bytes(), b"$2\r\nhi\r\n");
        assert_eq!(Reply::Array(vec![]).to_bytes(), b"*0\r\n");
    }
}
