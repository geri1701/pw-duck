use crate::analysis::AtomicF32;
use crate::logging::logln;
use pipewire as pw;
use pw::spa::param::audio::{AudioFormat, AudioInfoRaw};
use pw::spa::param::format::{MediaSubtype, MediaType};
use pw::spa::param::format_utils;
use pw::spa::pod::Pod;
use pw::{properties::properties, spa};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct CaptureData {
    pub format: AudioInfoRaw,
}

#[allow(
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::default_trait_access,
    clippy::needless_pass_by_value,
    clippy::redundant_clone,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::uninlined_format_args
)]
pub fn setup_capture(
    core: &pw::core::CoreRc,
    voice_id_opt: Option<u32>,
    voice_node_opt: Option<String>,
    voice_serial_opt: Option<String>,
    energy_atomic: Arc<AtomicF32>,
    audio_seen: Arc<AtomicBool>,
    capture_frames: Arc<AtomicU64>,
    gui_enabled: bool,
) -> Result<
    Option<(
        pw::stream::StreamBox<'_>,
        pw::stream::StreamListener<CaptureData>,
    )>,
    pw::Error,
> {
    voice_id_opt
        .map(|voice_id| {
            let mut props = properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Communication",
                *pw::keys::MEDIA_CLASS => "Stream/Input/Audio",
            };
            // target id
            if let Some(serial) = voice_serial_opt.clone().filter(|v| v != "-") {
                props.insert("target.object", serial);
            } else if let Some(node_name) = voice_node_opt.clone().filter(|v| v != "unknown-node") {
                props.insert("target.object", node_name);
            }
            // monitor capture
            props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
            let stream = pw::stream::StreamBox::new(core, "voice-capture", props)?;
            let user_data = CaptureData {
                format: Default::default(),
            };
            let energy_clone = energy_atomic.clone();
            let audio_seen_rt = audio_seen.clone();
            let capture_frames_rt = capture_frames.clone();
            let stream_listener = stream
                .add_local_listener_with_user_data(user_data)
                .param_changed(move |_, user_data, id, param| {
                    let Some(param) = param else {
                        return;
                    };
                    if id != pw::spa::param::ParamType::Format.as_raw() {
                        return;
                    }
                    let Ok((media_type, media_subtype)) = format_utils::parse_format(param) else {
                        return;
                    };
                    if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                        return;
                    }
                    user_data
                        .format
                        .parse(param)
                        .expect("Failed to parse audio format");
                    logln(
                        gui_enabled,
                        format!(
                            "Überwachung gestartet: rate={} channels={} format={:?}",
                            user_data.format.rate(),
                            user_data.format.channels(),
                            user_data.format.format()
                        ),
                    );
                })
                .process(move |stream, user_data| match stream.dequeue_buffer() {
                    None => (),
                    Some(mut buffer) => {
                        let datas = buffer.datas_mut();
                        if datas.is_empty() {
                            return;
                        }
                        let data = &mut datas[0];

                        let n_channels = user_data.format.channels();
                        if n_channels == 0 {
                            return;
                        }

                        let chunk = data.chunk();
                        let offset = chunk.offset() as usize;
                        let size = chunk.size() as usize;
                        if size == 0 {
                            return;
                        }

                        if let Some(samples) = data.data() {
                            if offset >= samples.len() {
                                return;
                            }
                            let end = (offset + size).min(samples.len());
                            if end <= offset {
                                return;
                            }

                            let slice = &samples[offset..end];

                            let mut sum_sq: f32 = 0.0;
                            let mut count: usize = 0;

                            match user_data.format.format() {
                                AudioFormat::F32LE => {
                                    for chunk in slice.chunks_exact(std::mem::size_of::<f32>()) {
                                        let sample = f32::from_le_bytes(chunk.try_into().unwrap());
                                        sum_sq += sample * sample;
                                        count += 1;
                                    }
                                }
                                AudioFormat::F32BE => {
                                    for chunk in slice.chunks_exact(std::mem::size_of::<f32>()) {
                                        let sample = f32::from_be_bytes(chunk.try_into().unwrap());
                                        sum_sq += sample * sample;
                                        count += 1;
                                    }
                                }
                                AudioFormat::S16LE => {
                                    for chunk in slice.chunks_exact(std::mem::size_of::<i16>()) {
                                        let sample = i16::from_le_bytes(chunk.try_into().unwrap())
                                            as f32
                                            / i16::MAX as f32;
                                        sum_sq += sample * sample;
                                        count += 1;
                                    }
                                }
                                AudioFormat::S16BE => {
                                    for chunk in slice.chunks_exact(std::mem::size_of::<i16>()) {
                                        let sample = i16::from_be_bytes(chunk.try_into().unwrap())
                                            as f32
                                            / i16::MAX as f32;
                                        sum_sq += sample * sample;
                                        count += 1;
                                    }
                                }
                                _ => {
                                    energy_clone.store(0.0);
                                }
                            }

                            if count > 0 {
                                let rms = (sum_sq / (count as f32)).sqrt();
                                energy_clone.store(rms);
                            }

                            audio_seen_rt.store(true, Ordering::Relaxed);
                            capture_frames_rt.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                })
                .register()?;

            let mut audio_info = AudioInfoRaw::new();
            audio_info.set_format(AudioFormat::F32LE);
            let obj = pw::spa::pod::Object {
                type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
                id: pw::spa::param::ParamType::EnumFormat.as_raw(),
                properties: audio_info.into(),
            };
            let serialized = pw::spa::pod::serialize::PodSerializer::serialize(
                std::io::Cursor::new(Vec::new()),
                &pw::spa::pod::Value::Object(obj),
            )
            .unwrap()
            .0
            .into_inner();

            let mut params = [Pod::from_bytes(&serialized).unwrap()];

            logln(
                gui_enabled,
                format!("Capture-Stream verbunden mit Voice-Node id={}", voice_id),
            );

            stream.connect(
                spa::utils::Direction::Input,
                None,
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut params,
            )?;

            Ok::<_, pw::Error>((stream, stream_listener))
        })
        .transpose()
}

#[allow(
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::default_trait_access,
    clippy::needless_pass_by_value,
    clippy::redundant_clone,
    clippy::too_many_lines
)]
pub fn probe_candidate_energy(
    mainloop: &pw::main_loop::MainLoopRc,
    core: &pw::core::CoreRc,
    target_node: Option<String>,
    target_serial: Option<String>,
    duration: Duration,
) -> Result<f32, pw::Error> {
    let energy_atomic = Arc::new(AtomicF32::new(0.0));
    let audio_seen = Arc::new(AtomicBool::new(false));
    let capture_frames = Arc::new(AtomicU64::new(0));
    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::MEDIA_CLASS => "Stream/Input/Audio",
    };
    if let Some(serial) = target_serial.clone().filter(|v| v != "-") {
        props.insert("target.object", serial);
    } else if let Some(node_name) = target_node.clone().filter(|v| v != "unknown-node") {
        props.insert("target.object", node_name);
    }
    props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
    let stream = pw::stream::StreamBox::new(core, "voice-probe", props)?;
    let user_data = CaptureData {
        format: Default::default(),
    };
    let energy_clone = energy_atomic.clone();
    let audio_seen_rt = audio_seen.clone();
    let capture_frames_rt = capture_frames.clone();
    let stream_listener = stream
        .add_local_listener_with_user_data(user_data)
        .param_changed(|_, user_data, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) = format_utils::parse_format(param) else {
                return;
            };
            if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                return;
            }
            user_data
                .format
                .parse(param)
                .expect("Failed to parse audio format");
        })
        .process(move |stream, user_data| match stream.dequeue_buffer() {
            None => (),
            Some(mut buffer) => {
                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }
                let data = &mut datas[0];

                let n_channels = user_data.format.channels();
                if n_channels == 0 {
                    return;
                }

                let chunk = data.chunk();
                let offset = chunk.offset() as usize;
                let size = chunk.size() as usize;
                if size == 0 {
                    return;
                }

                if let Some(samples) = data.data() {
                    if offset >= samples.len() {
                        return;
                    }
                    let end = (offset + size).min(samples.len());
                    if end <= offset {
                        return;
                    }

                    let slice = &samples[offset..end];
                    let mut sum_sq: f32 = 0.0;
                    let mut count: usize = 0;

                    match user_data.format.format() {
                        AudioFormat::F32LE => {
                            for chunk in slice.chunks_exact(std::mem::size_of::<f32>()) {
                                let sample = f32::from_le_bytes(chunk.try_into().unwrap());
                                sum_sq += sample * sample;
                                count += 1;
                            }
                        }
                        AudioFormat::F32BE => {
                            for chunk in slice.chunks_exact(std::mem::size_of::<f32>()) {
                                let sample = f32::from_be_bytes(chunk.try_into().unwrap());
                                sum_sq += sample * sample;
                                count += 1;
                            }
                        }
                        AudioFormat::S16LE => {
                            for chunk in slice.chunks_exact(std::mem::size_of::<i16>()) {
                                let sample = i16::from_le_bytes(chunk.try_into().unwrap()) as f32
                                    / i16::MAX as f32;
                                sum_sq += sample * sample;
                                count += 1;
                            }
                        }
                        AudioFormat::S16BE => {
                            for chunk in slice.chunks_exact(std::mem::size_of::<i16>()) {
                                let sample = i16::from_be_bytes(chunk.try_into().unwrap()) as f32
                                    / i16::MAX as f32;
                                sum_sq += sample * sample;
                                count += 1;
                            }
                        }
                        _ => {
                            energy_clone.store(0.0);
                        }
                    }

                    if count > 0 {
                        let rms = (sum_sq / (count as f32)).sqrt();
                        energy_clone.store(rms);
                    }

                    audio_seen_rt.store(true, Ordering::Relaxed);
                    capture_frames_rt.fetch_add(1, Ordering::Relaxed);
                }
            }
        })
        .register()?;

    let mut audio_info = AudioInfoRaw::new();
    audio_info.set_format(AudioFormat::F32LE);
    let obj = pw::spa::pod::Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pw::spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let serialized = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&serialized).unwrap()];
    stream.connect(
        spa::utils::Direction::Input,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;

    let max_energy = Rc::new(RefCell::new(0.0_f32));
    let max_energy_t = max_energy.clone();
    let energy_t = energy_atomic.clone();
    let start = Instant::now();
    let mainloop_t = mainloop.clone();
    let loop_handle = mainloop.loop_();
    let timer = loop_handle.add_timer(move |_| {
        let current = energy_t.load();
        let mut max_val = max_energy_t.borrow_mut();
        if current > *max_val {
            *max_val = current;
        }
        if start.elapsed() >= duration {
            mainloop_t.quit();
        }
    });
    timer
        .update_timer(
            Some(Duration::from_millis(50)),
            Some(Duration::from_millis(50)),
        )
        .into_result()?;
    mainloop.run();
    drop(stream_listener);
    drop(stream);

    let result = *max_energy.borrow();
    Ok(result)
}
