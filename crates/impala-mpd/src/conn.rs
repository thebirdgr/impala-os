//! One blocking TCP connection to MPD.

use crate::protocol::{ack_message, split_pair, Pairs};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Debug)]
pub enum ConnError {
    Io(io::Error),
    /// MPD answered `ACK …`. The connection is still usable.
    Ack(String),
    Protocol(String),
}

impl From<io::Error> for ConnError {
    fn from(e: io::Error) -> Self {
        ConnError::Io(e)
    }
}

pub struct Connection {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    pub server_version: String,
}

/// Upper bound for one album-art image.
const MAX_BINARY: usize = 8 * 1024 * 1024;

impl Connection {
    pub fn connect(addr: &str, timeout: Duration) -> Result<Self, ConnError> {
        let mut last_err = io::Error::new(io::ErrorKind::NotFound, format!("could not resolve {addr}"));
        for sa in addr.to_socket_addrs()? {
            match TcpStream::connect_timeout(&sa, timeout) {
                Ok(stream) => {
                    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
                    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
                    stream.set_nodelay(true)?;
                    let writer = stream.try_clone()?;
                    let mut reader = BufReader::new(stream);
                    let mut greeting = String::new();
                    reader.read_line(&mut greeting)?;
                    let version = greeting
                        .trim_end()
                        .strip_prefix("OK MPD ")
                        .ok_or_else(|| ConnError::Protocol(format!("unexpected greeting {greeting:?}")))?
                        .to_string();
                    return Ok(Self { reader, writer, server_version: version });
                }
                Err(e) => last_err = e,
            }
        }
        Err(ConnError::Io(last_err))
    }

    fn read_line(&mut self) -> Result<String, ConnError> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line)?;
        if n == 0 {
            return Err(ConnError::Io(io::Error::new(io::ErrorKind::UnexpectedEof, "MPD closed the connection")));
        }
        while line.ends_with('\n') || line.ends_with('\r') {
            line.pop();
        }
        Ok(line)
    }

    /// Send one command (already quoted) and collect `key: value` pairs.
    pub fn command(&mut self, cmd: &str) -> Result<Pairs, ConnError> {
        self.command_binary(cmd).map(|(p, _)| p)
    }

    /// Like [`Connection::command`], also returning a `binary:` payload.
    pub fn command_binary(&mut self, cmd: &str) -> Result<(Pairs, Option<Vec<u8>>), ConnError> {
        debug_assert!(!cmd.contains('\n'));
        self.send(cmd)?;
        self.read_response()
    }

    fn send(&mut self, text: &str) -> Result<(), ConnError> {
        let mut buf = Vec::with_capacity(text.len() + 1);
        buf.extend_from_slice(text.as_bytes());
        buf.push(b'\n');
        self.writer.write_all(&buf)?;
        Ok(())
    }

    fn read_response(&mut self) -> Result<(Pairs, Option<Vec<u8>>), ConnError> {
        let mut pairs = Vec::new();
        let mut binary = None;
        loop {
            let line = self.read_line()?;
            if line == "OK" {
                return Ok((pairs, binary));
            }
            if line.starts_with("ACK ") {
                return Err(ConnError::Ack(ack_message(&line)));
            }
            let (k, v) = split_pair(&line).ok_or_else(|| ConnError::Protocol(format!("bad line {line:?}")))?;
            if k == "binary" {
                let n: usize = v.parse().map_err(|_| ConnError::Protocol(format!("bad binary size {v}")))?;
                if n > MAX_BINARY {
                    return Err(ConnError::Protocol(format!("binary chunk too large: {n}")));
                }
                let mut buf = vec![0u8; n];
                self.reader.read_exact(&mut buf)?;
                let mut nl = [0u8; 1];
                self.reader.read_exact(&mut nl)?;
                binary = Some(buf);
            } else {
                pairs.push((k, v));
            }
        }
    }

    /// Run several commands atomically (`command_list_begin … end`).
    pub fn command_list(&mut self, cmds: &[String]) -> Result<Pairs, ConnError> {
        let mut text = String::from("command_list_begin\n");
        for c in cmds {
            debug_assert!(!c.contains('\n'));
            text.push_str(c);
            text.push('\n');
        }
        text.push_str("command_list_end");
        self.send(&text)?;
        self.read_response().map(|(p, _)| p)
    }
}
