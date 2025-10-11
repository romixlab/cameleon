use anyhow::Context;
use cameleon::gige::enumerate_cameras;
use std::net::Ipv4Addr;

fn main() -> anyhow::Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    let local_addr = args
        .get(1)
        .context("No IP address argument provided")?
        .parse::<Ipv4Addr>()?;
    let mut cameras = enumerate_cameras(local_addr)?;
    if cameras.is_empty() {
        println!("No cameras found!");
        return Ok(());
    }

    for (idx, camera) in cameras.iter_mut().enumerate() {
        camera.open()?;
        println!("{idx}: {:?}", camera.ctrl.device_info());
        // let xml = camera.load_context().unwrap();
        // println!("{}", xml);
        camera.close()?;
    }

    Ok(())
}
