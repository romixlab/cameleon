/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

pub mod protocol;
pub mod register_map;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

pub const GVCP_DEFAULT_PORT: u16 = 3956;

use std::net::Ipv4Addr;
use std::net::UdpSocket;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tracing::warn;

use crate::gige::protocol::PacketStatus;
use protocol::{ack, cmd, prelude::*};

#[tracing::instrument(level = "warn")]
pub fn enumerate_devices(local_addr: Ipv4Addr, timeout: Duration) -> Result<Vec<ack::Discovery>> {
    let mut buf = [0_u8; 1024];
    let sock = broadcast_discovery_packet(local_addr, &mut buf)?;
    sock.set_read_timeout(Some(timeout))?;

    let mut discoveries = vec![];
    let discovery_start = Instant::now();
    while sock.recv(&mut buf).is_ok() {
        if let Ok(ack) = ack::AckPacket::parse(&buf) {
            if ack.status().is_success() {
                match ack.ack_data_as::<ack::Discovery>() {
                    Ok(discovery) => discoveries.push(discovery),
                    Err(err) => warn!("{}", err),
                }
            } else {
                warn!("invalid discovery ack status: {:?}", ack.status())
            }
        }
        match timeout.checked_sub(Instant::now().duration_since(discovery_start)) {
            Some(remaining) => {
                sock.set_read_timeout(Some(remaining))?;
            }
            None => break,
        }
    }

    Ok(discoveries)
}

pub fn discovery_worker(
    local_addr: Ipv4Addr,
    event_tx: mpsc::Sender<Result<ack::Discovery>>,
    stop: oneshot::Receiver<()>,
) {
    let mut buf = [0_u8; 1024];
    let sock = match broadcast_discovery_packet(local_addr, &mut buf) {
        Ok(sock) => sock,
        Err(e) => {
            _ = event_tx.send(Err(e));
            return;
        }
    };
    if let Err(e) = sock.set_read_timeout(Some(Duration::from_millis(250))) {
        _ = event_tx.send(Err(e.into()));
        return;
    }

    loop {
        match sock.recv(&mut buf) {
            Ok(_ack) => {
                if let Ok(ack) = ack::AckPacket::parse(&buf) {
                    if ack.status().is_success() {
                        match ack.ack_data_as::<ack::Discovery>() {
                            Ok(discovery) => {
                                let _ = event_tx.send(Ok(discovery));
                            }
                            Err(e) => {
                                warn!("{}", e);
                                _ = event_tx.send(Err(e));
                            }
                        }
                    } else {
                        warn!("invalid discovery ack status: {:?}", ack.status());
                        _ = event_tx.send(Err(Error::InvalidAckStatus(ack.status())));
                    }
                }
            }
            Err(e) => {
                if stop.try_recv().is_ok() {
                    return;
                }
                if e.kind() == std::io::ErrorKind::TimedOut {
                    continue;
                }
                _ = event_tx.send(Err(e.into()));
                return;
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("packet is broken: {0}")]
    InvalidPacket(std::borrow::Cow<'static, str>),

    #[error("invalid data: {0}")]
    InvalidData(std::borrow::Cow<'static, str>),

    #[error("invalid ack status: {0:?}")]
    InvalidAckStatus(PacketStatus),
}

fn broadcast_discovery_packet(local_addr: Ipv4Addr, buf: &mut [u8]) -> Result<UdpSocket> {
    let sock = UdpSocket::bind((local_addr, 0))?;
    let packet = cmd::Discovery::new().finalize(0xffff);
    packet.serialize(buf.as_mut())?;
    let length = packet.length() as usize;

    sock.set_broadcast(true)?;
    sock.send_to(&buf[..length], ("255.255.255.255", GVCP_DEFAULT_PORT))?;
    sock.set_broadcast(false)?;
    Ok(sock)
}
