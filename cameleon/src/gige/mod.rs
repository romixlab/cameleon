/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

pub mod control_handle;
pub mod register_map;
pub mod stream_handle;

pub use control_handle::ControlHandle;
pub use stream_handle::StreamHandle;

use std::{net::Ipv4Addr, time};

use cameleon_device::gige::{self};

use crate::ControlError;

use super::{CameleonResult, Camera};

const ENUMERATION_TIMEOUT: time::Duration = time::Duration::from_millis(500);

impl From<gige::Error> for ControlError {
    fn from(err: gige::Error) -> Self {
        match err {
            gige::Error::Io(err) => ControlError::Io(err.into()),
            gige::Error::InvalidPacket(msg) => ControlError::InvalidData(msg.into()),
            gige::Error::InvalidData(msg) => ControlError::InvalidData(msg.into()),
            gige::Error::InvalidAckStatus(status) => {
                ControlError::Io(anyhow::anyhow!("{:?}", status))
            }
        }
    }
}

// TODO: do not bind here, but only when camera is actually needed
pub fn enumerate_cameras(
    local_addr: Ipv4Addr,
) -> CameleonResult<Vec<Camera<ControlHandle, StreamHandle>>> {
    let device_infos =
        gige::enumerate_devices(local_addr, ENUMERATION_TIMEOUT).map_err(ControlError::from)?;

    let mut cameras: Vec<Camera<ControlHandle, StreamHandle>> =
        Vec::with_capacity(device_infos.len());
    for discovery in device_infos {
        cameras.push(Camera::from_discovery(discovery, local_addr)?);
    }

    Ok(cameras)
}
