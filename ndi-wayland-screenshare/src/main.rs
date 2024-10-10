use std::{os::fd::OwnedFd, thread::sleep, time::{Duration, Instant}};

use anyhow::Result;
use ashpd::{
    desktop::screencast::{CursorMode, PersistMode, Screencast, SourceType},
    WindowIdentifier,
};
use crossbeam_channel::{Receiver, Sender};
use ndi::NdiLib;
use pipewire as pw;
use pipewire::{main_loop::MainLoop, spa};

struct UserData {
    format: spa::param::video::VideoInfoRaw,
}

struct OwnedFrame {
    format: spa::param::video::VideoInfoRaw,
    create_time: Instant,
    data: Vec<u8>,
}

fn ndi_loop(rx: Receiver<OwnedFrame>) -> Result<()> {
    let ndi_lib = NdiLib::new()?;
    let sender = ndi_lib.create_sender(Some("Desktop"), None, false, false)?;

    loop {
        let mut last_frame = rx.recv()?;

        if last_frame.create_time.elapsed() > Duration::from_millis(100) {
            println!("Frame too old, skipping");
            continue;
        }

        sender.send(ndi::Frame {
            width: last_frame.format.size().width,
            height: last_frame.format.size().height,
            format: ndi::VideoFormat::BGRX,
            data: &mut last_frame.data,
            stride_in_bytes: last_frame.format.size().width * 4,
        });
    }
}

fn pipewire_loop(fd: OwnedFd, node_id: u32, tx: Sender<OwnedFrame>) -> anyhow::Result<()> {
    let main_loop = MainLoop::new(None)?;
    let ctx = pipewire::context::Context::new(&main_loop)?;
    let core = ctx.connect_fd(fd, None)?;

    let data = UserData {
        format: Default::default(),
    };

    let stream = pipewire::stream::Stream::new(
        &core,
        "video-capture",
        pipewire::properties::properties! {
            *pipewire::keys::MEDIA_TYPE => "Video",
            *pipewire::keys::MEDIA_CATEGORY => "Capture",
            *pipewire::keys::MEDIA_ROLE => "Screen",
        },
    )?;

    let _listener = stream
        .add_local_listener_with_user_data(data)
        .state_changed(|_, _, old, new| {
            println!("State changed: {:?} -> {:?}", old, new);
        })
        .param_changed(|_, user_data, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }

            let (media_type, media_subtype) =
                match pw::spa::param::format_utils::parse_format(param) {
                    Ok(v) => v,
                    Err(_) => return,
                };

            if media_type != pw::spa::param::format::MediaType::Video
                || media_subtype != pw::spa::param::format::MediaSubtype::Raw
            {
                return;
            }

            user_data
                .format
                .parse(param)
                .expect("Failed to parse param changed to VideoInfoRaw");

            println!("got video format:");
            println!(
                "  format: {} ({:?})",
                user_data.format.format().as_raw(),
                user_data.format.format()
            );
            println!(
                "  size: {}x{}",
                user_data.format.size().width,
                user_data.format.size().height
            );
            println!(
                "  framerate: {}/{}",
                user_data.format.framerate().num,
                user_data.format.framerate().denom
            );

            // prepare to render video of this size
        })
        .process(move |stream, user_data| {
            match stream.dequeue_buffer() {
                None => println!("out of buffers"),
                Some(mut buffer) => {
                    let datas = buffer.datas_mut();
                    if datas.is_empty() {
                        return;
                    }

                    // copy frame data to screen
                    let data = if let Some(d) = datas[0].data() {
                        d
                    } else {
                        return;
                    };

                    let frame = OwnedFrame {
                        format: user_data.format,
                        create_time: Instant::now(),
                        data: data.to_vec(),
                    };

                    tx.send(frame).ok();
                }
            }
        })
        .register()?;

    let obj = pw::spa::pod::object!(
        pw::spa::utils::SpaTypes::ObjectParamFormat,
        pw::spa::param::ParamType::EnumFormat,
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaType,
            Id,
            pw::spa::param::format::MediaType::Video
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaSubtype,
            Id,
            pw::spa::param::format::MediaSubtype::Raw
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            pw::spa::param::video::VideoFormat::BGRA,
            pw::spa::param::video::VideoFormat::RGBA,
            pw::spa::param::video::VideoFormat::RGBx,
            pw::spa::param::video::VideoFormat::BGRx,
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            pw::spa::utils::Rectangle {
                width: 320,
                height: 240
            },
            pw::spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            pw::spa::utils::Rectangle {
                width: 10240,
                height: 10240
            }
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            pw::spa::utils::Fraction { num: 60, denom: 1 },
            pw::spa::utils::Fraction { num: 0, denom: 1 },
            pw::spa::utils::Fraction {
                num: 1000,
                denom: 1
            }
        ),
    );
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner();

    let mut params = [spa::pod::Pod::from_bytes(&values).unwrap()];

    stream.connect(
        spa::utils::Direction::Input,
        Some(node_id),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;

    main_loop.run();

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let proxy = Screencast::new().await?;
    let session = proxy.create_session().await?;
    proxy
        .select_sources(
            &session,
            CursorMode::Embedded,
            SourceType::Monitor | SourceType::Window,
            true,
            None,
            PersistMode::DoNot,
        )
        .await?;
    let response = proxy
        .start(&session, &WindowIdentifier::default())
        .await?
        .response()?;

    let stream = response.streams().iter().next().unwrap();
    let node_id = stream.pipe_wire_node_id();
    let fd = proxy.open_pipe_wire_remote(&session).await?;

    let (tx, rx) = crossbeam_channel::unbounded();

    let pw_thread = std::thread::spawn(move || {
        if let Err(e) = pipewire_loop(fd, node_id, tx) {
            eprintln!("Error: {}", e);
        }
    });
    let ndi_thread = std::thread::spawn(move || {
        if let Err(e) = ndi_loop(rx) {
            eprintln!("Error: {}", e);
        }
    });

    ndi_thread.join().unwrap();
    pw_thread.join().unwrap();

    Ok(())
}
