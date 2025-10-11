use cameleon::{
    Camera,
    gige::{ControlHandle, StreamHandle},
    payload::{ImageInfo, Payload, PayloadReceiver},
};
use cameleon_device::PixelFormat;
use egui::{Button, CentralPanel, ColorImage, ComboBox, Label, TextureHandle, TopBottomPanel, Ui};
use image::{ImageBuffer, Rgb};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::net::IpAddr;
use std::{io::Cursor, net::Ipv4Addr, time::Instant};

#[tokio::main]
async fn main() {
    let filter = tracing_subscriber::EnvFilter::from_default_env();
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 960.0]),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "GigE streaming example",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(StreamingExample::new(cc)))
        }),
    )
    .unwrap();
}

struct FpsCounter {
    timestamp: Instant,
    fps_count: u64,
    avg: Option<f64>,
}

impl FpsCounter {
    fn new() -> Self {
        Self {
            timestamp: Instant::now(),
            fps_count: 0,
            avg: None,
        }
    }

    pub fn bump(&mut self) {
        self.fps_count += 1;
        let delta = Instant::now() - self.timestamp;
        if delta.as_secs() > 1 {
            self.avg = Some(self.fps_count as f64 / delta.as_secs_f64());
            self.timestamp = Instant::now();
            self.fps_count = 0;
        }
    }
}

impl std::fmt::Display for FpsCounter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.avg {
            None => f.write_str("N/A"),
            Some(avg) => f.write_fmt(format_args!("{:.02}", avg)),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
struct NamedInterface {
    name: String,
    addr: Ipv4Addr,
}

impl Display for NamedInterface {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} - {}", self.name, self.addr)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct StreamingExample {
    if_addr: Option<NamedInterface>,
    #[serde(skip)]
    transient: Option<Transient>,
}

struct Transient {
    handle: TextureHandle,
    cam: Option<(Camera<ControlHandle, StreamHandle>, PayloadReceiver)>,
    last_im: Option<ImageInfo>,
    fps: Option<FpsCounter>,
    if_addrs: Vec<NamedInterface>,
}

impl StreamingExample {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let mut s = if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_else(|| Self::default())
        } else {
            Self::default()
        };

        let mut if_addrs = get_if_addrs::get_if_addrs().unwrap_or_default();
        if_addrs.retain(|iface| !iface.is_loopback());
        let if_addrs = if_addrs
            .drain(..)
            .filter_map(|iface| {
                if iface.is_loopback() {
                    None
                } else if let IpAddr::V4(v4_addr) = iface.ip() {
                    Some(NamedInterface {
                        name: iface.name,
                        addr: v4_addr,
                    })
                } else {
                    None
                }
            })
            .collect();

        s.transient = Some(Transient {
            handle: cc.egui_ctx.load_texture(
                "s",
                rgb2egui(&ImageBuffer::from_vec(1, 1, vec![1, 1, 1]).unwrap()),
                egui::TextureOptions::LINEAR,
            ),
            cam: None,
            last_im: None,
            fps: None,
            if_addrs,
        });
        s
    }

    fn start_stop(t: &mut Transient, if_addr: Option<Ipv4Addr>, ui: &mut Ui) {
        if ui
            .add_enabled(if_addr.is_some(), Button::new("Start"))
            .clicked()
            && t.cam.is_none()
        {
            t.cam = Some(get_camera(if_addr.expect("")));
            t.fps = Some(FpsCounter::new());
        }
        if ui.button("Stop").clicked() && t.cam.is_some() {
            let (mut cam, _) = t.cam.take().unwrap();
            cam.stop_streaming().unwrap();
            cam.close().unwrap();
            t.cam = None;
            t.last_im = None;
            t.fps = None;
        }
    }

    fn stats(t: &mut Transient, ui: &mut Ui) {
        if let Some(im) = t.last_im.as_ref() {
            ui.add(Label::new(format!(
                "{}x{} {:?}",
                im.width, im.height, im.pixel_format
            )));
        }
        if let Some(fps) = t.fps.as_ref() {
            ui.add(Label::new(format!("{} fps", fps)));
        }
    }

    fn interface_selection(t: &mut Transient, if_addr: &mut Option<NamedInterface>, ui: &mut Ui) {
        ui.label("Bind to:");
        let selected_text = if let Some(if_addr) = if_addr {
            format!("{if_addr}")
        } else {
            "Select interface...".into()
        };
        ComboBox::from_id_salt("if_addr")
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                for name_addr in &t.if_addrs {
                    ui.selectable_value(if_addr, Some(name_addr.clone()), format!("{}", name_addr));
                }
            })
            .response
            .on_hover_text(
                "This is NOT the IP address of the camera, but a local interface to which to bind",
            );
    }
}

impl eframe::App for StreamingExample {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let Some(t) = &mut self.transient else {
            return;
        };

        TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                Self::interface_selection(t, &mut self.if_addr, ui);
                Self::start_stop(t, self.if_addr.as_ref().map(|n| n.addr), ui);
                Self::stats(t, ui);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::warn_if_debug_build(ui);
                });
            });
        });

        CentralPanel::default().show(ctx, |ui| {
            let txt = egui::load::SizedTexture::from_handle(&t.handle);
            ui.add(egui::Image::from_texture(txt).shrink_to_fit());

            let Some((_, prx)) = t.cam.as_ref() else {
                return;
            };
            let buf = prx.try_recv();
            let Ok(buf) = buf else {
                return;
            };
            if let Some(fps) = t.fps.as_mut() {
                fps.bump();
            }
            t.last_im = Some(buf.image_info().unwrap().clone());
            let rgb = cameleon2rgb(buf);
            let img = rgb2egui(&rgb);
            t.handle.set(img, egui::TextureOptions::LINEAR);
        });
        ctx.request_repaint();
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}

fn cameleon2rgb(buf: Payload) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let mut raw = Cursor::new(buf.payload());
    let mut rgb = vec![0u8; buf.payload().len() * 3];
    let ii = buf.image_info().unwrap();
    assert_eq!(ii.width * ii.height, buf.payload().len());
    assert_eq!(ii.pixel_format, PixelFormat::BayerRG8);
    let mut raster =
        bayer::RasterMut::new(ii.width, ii.height, bayer::RasterDepth::Depth8, &mut rgb);
    bayer::demosaic(
        &mut raw,
        bayer::BayerDepth::Depth8,
        bayer::CFA::RGGB,
        bayer::Demosaic::Linear,
        &mut raster,
    )
    .unwrap();
    let buffer: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_vec(ii.width as u32, ii.height as u32, rgb).unwrap();
    buffer
}

fn rgb2egui(rgb: &ImageBuffer<Rgb<u8>, Vec<u8>>) -> ColorImage {
    ColorImage::from_rgb([rgb.width() as usize, rgb.height() as usize], rgb)
}

fn get_camera(ip_addr: Ipv4Addr) -> (Camera<ControlHandle, StreamHandle>, PayloadReceiver) {
    let mut camera = cameleon::gige::enumerate_cameras(ip_addr)
        .unwrap()
        .swap_remove(0);
    camera.open().unwrap();
    camera.load_context().unwrap();
    let mut ctxt = camera.params_ctxt().unwrap();

    ctxt.node("GainAuto")
        .unwrap()
        .as_enumeration(&ctxt)
        .unwrap()
        .set_entry_by_symbolic(&mut ctxt, "Continuous")
        .unwrap();

    let prx = camera.start_streaming(3).unwrap();
    (camera, prx)
}
