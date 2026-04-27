use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::UdpSocket;

use crate::error::Error;
use crate::osc::{self, OscMessage, OscPacket};

type Handler = Arc<dyn Fn(OscMessage, SocketAddr) + Send + Sync + 'static>;

pub struct OscServer {
    socket: Arc<UdpSocket>,
    handlers: Vec<(String, Handler)>,
    buffer_size: usize,
}

impl OscServer {
    pub async fn bind(addr: impl tokio::net::ToSocketAddrs) -> Result<Self, Error> {
        let socket = UdpSocket::bind(addr).await?;
        Ok(Self {
            socket: Arc::new(socket),
            handlers: Vec::new(),
            buffer_size: 65536,
        })
    }

    pub fn handle<F>(&mut self, pattern: impl Into<String>, f: F)
    where
        F: Fn(OscMessage, SocketAddr) + Send + Sync + 'static,
    {
        self.handlers.push((pattern.into(), Arc::new(f)));
    }

    pub fn set_buffer_size(&mut self, size: usize) {
        self.buffer_size = size;
    }

    pub fn local_addr(&self) -> Result<SocketAddr, Error> {
        Ok(self.socket.local_addr()?)
    }

    pub async fn run(&self) -> Result<(), Error> {
        let mut buf = vec![0u8; self.buffer_size];
        loop {
            let (n, src) = self.socket.recv_from(&mut buf).await?;
            let data = &buf[..n];
            match osc::decode(data) {
                Ok(packet) => {
                    if let Err(e) = self.dispatch(packet, src) {
                        log::warn!("dispatch error: {}", e);
                    }
                }
                Err(e) => {
                    log::warn!("failed to decode OSC packet from {}: {}", src, e);
                }
            }
        }
    }

    fn dispatch(&self, packet: OscPacket, src: SocketAddr) -> Result<(), Error> {
        match packet {
            OscPacket::Message(msg) => {
                for (pattern, handler) in &self.handlers {
                    if osc::pattern_match(pattern, &msg.addr) {
                        handler(msg.clone(), src);
                    }
                }
                Ok(())
            }
            OscPacket::Bundle(bundle) => {
                for p in bundle.packets {
                    self.dispatch(p, src)?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc::{OscBundle, OscMessage, OscPacket, OscType, TimeTag};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_pattern_registration() {
        let server = OscServer::bind("127.0.0.1:0");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        let mut server = rt.block_on(server).unwrap();

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        server.handle("/test", move |_msg, _src| {
            called_clone.store(true, Ordering::SeqCst);
        });

        assert_eq!(server.handlers.len(), 1);

        let msg = OscMessage {
            addr: "/test".into(),
            args: vec![OscType::Int(42)],
        };

        server
            .dispatch(OscPacket::Message(msg), "127.0.0.1:0".parse().unwrap())
            .unwrap();

        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_dispatch_bundle_unwraps() {
        let server = OscServer::bind("127.0.0.1:0");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        let mut server = rt.block_on(server).unwrap();

        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count_clone = count.clone();

        server.handle("/*", move |_msg, _src| {
            count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });

        let bundle = OscBundle {
            timetag: TimeTag::immediate(),
            packets: vec![
                OscPacket::Message(OscMessage {
                    addr: "/a".into(),
                    args: vec![],
                }),
                OscPacket::Message(OscMessage {
                    addr: "/b".into(),
                    args: vec![],
                }),
            ],
        };

        server
            .dispatch(OscPacket::Bundle(bundle), "127.0.0.1:0".parse().unwrap())
            .unwrap();

        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn test_no_handler_match() {
        let server = OscServer::bind("127.0.0.1:0");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        let mut server = rt.block_on(server).unwrap();

        server.handle("/audio/volume", |_msg, _src| {});

        let msg = OscMessage {
            addr: "/mixer/pan".into(),
            args: vec![],
        };

        let result = server.dispatch(OscPacket::Message(msg), "127.0.0.1:0".parse().unwrap());
        assert!(result.is_ok());
    }

    #[test]
    fn test_set_buffer_size() {
        let server = OscServer::bind("127.0.0.1:0");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        let mut server = rt.block_on(server).unwrap();
        server.set_buffer_size(1024);
        assert_eq!(server.buffer_size, 1024);
    }

    #[test]
    fn test_local_addr() {
        let server = OscServer::bind("127.0.0.1:0");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        let server = rt.block_on(server).unwrap();
        let addr = server.local_addr().unwrap();
        assert!(addr.port() > 0);
    }
}
