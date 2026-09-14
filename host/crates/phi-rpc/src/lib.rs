//! Message framing for the ring's rpc channel (kind 3).
//!
//! The host daemon (`phictl serve`) and the card agent (`card/agent`)
//! exchange these messages as a byte stream in each direction. One
//! session runs at a time: a command with its input and output, or a file
//! transfer. The encoding is deliberately plain so that both ends, built by
//! different toolchains, agree byte for byte:
//!
//! ```text
//! frame   := u32 length (of what follows, little-endian) | u8 tag | body
//! string  := u16 length | bytes (UTF-8, at most 65535 bytes)
//! bytes   := the rest of the body
//! list    := u16 count | items
//! ```
//!
//! Nothing here touches the ring itself; `phi-ring` and `/dev/phirpc` carry
//! the frames. See lib.md.

use std::fmt;

/// Largest frame either side accepts (body plus tag).
pub const MAX_FRAME: usize = 1 << 20;

/// A message on the rpc channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    /// Host to card: start a command. `argv[0]` is the program.
    Exec {
        argv: Vec<String>,
        env: Vec<(String, String)>,
        cwd: Option<String>,
    },
    /// Host to card: bytes for the command's standard input.
    Stdin(Vec<u8>),
    /// Host to card: standard input is closed.
    StdinEof,
    /// Card to host: bytes the command wrote to standard output.
    Stdout(Vec<u8>),
    /// Card to host: bytes the command wrote to standard error.
    Stderr(Vec<u8>),
    /// Card to host: the command ended (exit status, or 128 + signal).
    Exit(i32),
    /// Host to card: create or truncate a file, then expect `PutData`/`PutClose`.
    PutOpen { path: String, mode: u32 },
    /// Host to card: file contents, in order.
    PutData(Vec<u8>),
    /// Host to card: the file is complete.
    PutClose,
    /// Host to card: send this file back as `GetData` frames, then `GetEnd`.
    Get { path: String },
    /// Card to host: file contents, in order.
    GetData(Vec<u8>),
    /// Card to host: the file is complete; `size` bytes were sent.
    GetEnd { size: u64 },
    /// Either direction: the request failed; the session is over.
    Error(String),
    /// Host to card: liveness check.
    Ping,
    /// Card to host: reply to `Ping`, with the agent's version.
    Pong { version: String },
}

/// Why a frame could not be decoded.
#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Frame length exceeds [`MAX_FRAME`] or is zero.
    Length(u32),
    /// Unknown message tag.
    Tag(u8),
    /// Body shorter than its fields require, or not UTF-8 where a string is.
    Malformed,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Length(n) => write!(f, "frame length {n} out of range"),
            DecodeError::Tag(t) => write!(f, "unknown message tag {t:#x}"),
            DecodeError::Malformed => write!(f, "malformed frame body"),
        }
    }
}

impl std::error::Error for DecodeError {}

const TAG_EXEC: u8 = 1;
const TAG_STDIN: u8 = 2;
const TAG_STDIN_EOF: u8 = 3;
const TAG_STDOUT: u8 = 4;
const TAG_STDERR: u8 = 5;
const TAG_EXIT: u8 = 6;
const TAG_PUT_OPEN: u8 = 7;
const TAG_PUT_DATA: u8 = 8;
const TAG_PUT_CLOSE: u8 = 9;
const TAG_GET: u8 = 10;
const TAG_GET_DATA: u8 = 11;
const TAG_GET_END: u8 = 12;
const TAG_ERROR: u8 = 13;
const TAG_PING: u8 = 14;
const TAG_PONG: u8 = 15;

fn put_str(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    let n = b.len().min(u16::MAX as usize);
    out.extend_from_slice(&(n as u16).to_le_bytes());
    out.extend_from_slice(&b[..n]);
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.pos + n > self.buf.len() {
            return Err(DecodeError::Malformed);
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, DecodeError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes(b.try_into().map_err(|_| DecodeError::Malformed)?))
    }
    fn str(&mut self) -> Result<String, DecodeError> {
        let n = self.u16()? as usize;
        let b = self.take(n)?;
        String::from_utf8(b.to_vec()).map_err(|_| DecodeError::Malformed)
    }
    fn rest(&mut self) -> Vec<u8> {
        let s = self.buf[self.pos..].to_vec();
        self.pos = self.buf.len();
        s
    }
}

impl Msg {
    /// Serialise as one frame (length prefix included).
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        let tag = match self {
            Msg::Exec { argv, env, cwd } => {
                body.extend_from_slice(&(argv.len().min(u16::MAX as usize) as u16).to_le_bytes());
                for a in argv.iter().take(u16::MAX as usize) {
                    put_str(&mut body, a);
                }
                body.extend_from_slice(&(env.len().min(u16::MAX as usize) as u16).to_le_bytes());
                for (k, v) in env.iter().take(u16::MAX as usize) {
                    put_str(&mut body, k);
                    put_str(&mut body, v);
                }
                put_str(&mut body, cwd.as_deref().unwrap_or(""));
                TAG_EXEC
            }
            Msg::Stdin(d) => {
                body.extend_from_slice(d);
                TAG_STDIN
            }
            Msg::StdinEof => TAG_STDIN_EOF,
            Msg::Stdout(d) => {
                body.extend_from_slice(d);
                TAG_STDOUT
            }
            Msg::Stderr(d) => {
                body.extend_from_slice(d);
                TAG_STDERR
            }
            Msg::Exit(code) => {
                body.extend_from_slice(&code.to_le_bytes());
                TAG_EXIT
            }
            Msg::PutOpen { path, mode } => {
                put_str(&mut body, path);
                body.extend_from_slice(&mode.to_le_bytes());
                TAG_PUT_OPEN
            }
            Msg::PutData(d) => {
                body.extend_from_slice(d);
                TAG_PUT_DATA
            }
            Msg::PutClose => TAG_PUT_CLOSE,
            Msg::Get { path } => {
                put_str(&mut body, path);
                TAG_GET
            }
            Msg::GetData(d) => {
                body.extend_from_slice(d);
                TAG_GET_DATA
            }
            Msg::GetEnd { size } => {
                body.extend_from_slice(&size.to_le_bytes());
                TAG_GET_END
            }
            Msg::Error(s) => {
                put_str(&mut body, s);
                TAG_ERROR
            }
            Msg::Ping => TAG_PING,
            Msg::Pong { version } => {
                put_str(&mut body, version);
                TAG_PONG
            }
        };
        let mut out = Vec::with_capacity(5 + body.len());
        out.extend_from_slice(&((1 + body.len()) as u32).to_le_bytes());
        out.push(tag);
        out.extend_from_slice(&body);
        out
    }

    /// Decode one frame body (tag plus fields, without the length prefix).
    fn decode_body(tag: u8, body: &[u8]) -> Result<Msg, DecodeError> {
        let mut r = Reader { buf: body, pos: 0 };
        let msg = match tag {
            TAG_EXEC => {
                let n = r.u16()? as usize;
                let mut argv = Vec::with_capacity(n.min(256));
                for _ in 0..n {
                    argv.push(r.str()?);
                }
                let n = r.u16()? as usize;
                let mut env = Vec::with_capacity(n.min(256));
                for _ in 0..n {
                    let k = r.str()?;
                    let v = r.str()?;
                    env.push((k, v));
                }
                let cwd = r.str()?;
                Msg::Exec {
                    argv,
                    env,
                    cwd: if cwd.is_empty() { None } else { Some(cwd) },
                }
            }
            TAG_STDIN => Msg::Stdin(r.rest()),
            TAG_STDIN_EOF => Msg::StdinEof,
            TAG_STDOUT => Msg::Stdout(r.rest()),
            TAG_STDERR => Msg::Stderr(r.rest()),
            TAG_EXIT => Msg::Exit(r.u32()? as i32),
            TAG_PUT_OPEN => {
                let path = r.str()?;
                let mode = r.u32()?;
                Msg::PutOpen { path, mode }
            }
            TAG_PUT_DATA => Msg::PutData(r.rest()),
            TAG_PUT_CLOSE => Msg::PutClose,
            TAG_GET => Msg::Get { path: r.str()? },
            TAG_GET_DATA => Msg::GetData(r.rest()),
            TAG_GET_END => Msg::GetEnd { size: r.u64()? },
            TAG_ERROR => Msg::Error(r.str()?),
            TAG_PING => Msg::Ping,
            TAG_PONG => Msg::Pong { version: r.str()? },
            t => return Err(DecodeError::Tag(t)),
        };
        Ok(msg)
    }
}

/// Incremental frame decoder over a byte stream.
#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append received bytes.
    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Bytes buffered but not yet forming a whole frame.
    pub fn pending(&self) -> usize {
        self.buf.len()
    }

    /// Take the next complete frame, if any. After an error the stream is
    /// unusable: the caller should end the session.
    pub fn next_frame(&mut self) -> Result<Option<Msg>, DecodeError> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]);
        if len == 0 || len as usize > MAX_FRAME {
            return Err(DecodeError::Length(len));
        }
        let total = 4 + len as usize;
        if self.buf.len() < total {
            return Ok(None);
        }
        let msg = Msg::decode_body(self.buf[4], &self.buf[5..total])?;
        self.buf.drain(..total);
        Ok(Some(msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_every_message() {
        let msgs = vec![
            Msg::Exec {
                argv: vec!["/bin/sh".into(), "-c".into(), "echo hi".into()],
                env: vec![("PATH".into(), "/bin".into())],
                cwd: Some("/tmp".into()),
            },
            Msg::Exec {
                argv: vec![],
                env: vec![],
                cwd: None,
            },
            Msg::Stdin(b"abc".to_vec()),
            Msg::StdinEof,
            Msg::Stdout(vec![0, 255, 7]),
            Msg::Stderr(vec![]),
            Msg::Exit(-1),
            Msg::PutOpen {
                path: "/opt/x".into(),
                mode: 0o755,
            },
            Msg::PutData(vec![1; 1000]),
            Msg::PutClose,
            Msg::Get {
                path: "/etc/passwd".into(),
            },
            Msg::GetData(vec![2; 10]),
            Msg::GetEnd { size: 1 << 40 },
            Msg::Error("no such file".into()),
            Msg::Ping,
            Msg::Pong { version: "0.1".into() },
        ];
        let mut stream = Vec::new();
        for m in &msgs {
            stream.extend_from_slice(&m.encode());
        }
        // Feed in odd-sized pieces to exercise reassembly.
        let mut d = Decoder::new();
        let mut got = Vec::new();
        for piece in stream.chunks(7) {
            d.push(piece);
            while let Some(m) = d.next_frame().unwrap() {
                got.push(m);
            }
        }
        assert_eq!(got, msgs);
        assert_eq!(d.pending(), 0);
    }

    #[test]
    fn rejects_bad_frames() {
        let mut d = Decoder::new();
        d.push(&[0, 0, 0, 0]);
        assert_eq!(d.next_frame(), Err(DecodeError::Length(0)));
        let mut d = Decoder::new();
        d.push(&[1, 0, 0, 0, 200]);
        assert_eq!(d.next_frame(), Err(DecodeError::Tag(200)));
        let mut d = Decoder::new();
        d.push(&[2, 0, 0, 0, TAG_GET, 5]);
        assert_eq!(d.next_frame(), Err(DecodeError::Malformed));
    }
}
