//! Native PipeWire microphone capture for Linux.
//!
//! PipeWire objects stay on one dedicated thread because they are not `Send`.
//! The existing recorder consumer remains responsible for resampling, VAD,
//! visualization, and buffering.

use std::{
    cell::RefCell,
    io::{Cursor, Error},
    mem,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

use pipewire as pw;
use pw::prelude::*;
use pw::spa;
use spa::{
    pod::{serialize::PodSerializer, Object, Property, PropertyFlags, Value},
    utils::Id,
    Direction,
};

use super::recorder::{
    run_consumer, AudioChunk, AudioFrameCallback, Cmd, LevelCallback, VadConfig, VadPolicy,
};

const APP_NAME: &str = "Handy Hybrid";
const NODE_NAME: &str = "handy-hybrid-capture";
const PREFERRED_SAMPLE_RATE: u32 = 48_000;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct PipeWireRecorder {
    vad: Option<VadConfig>,
    level_cb: Option<LevelCallback>,
    audio_cb: Option<AudioFrameCallback>,
    cmd_tx: Option<mpsc::Sender<Cmd>>,
    quit_tx: Option<pw::channel::Sender<()>>,
    pipewire_handle: Option<std::thread::JoinHandle<()>>,
    consumer_handle: Option<std::thread::JoinHandle<()>>,
    healthy: Arc<AtomicBool>,
}

impl PipeWireRecorder {
    pub(crate) fn from_parts(
        vad: Option<VadConfig>,
        level_cb: Option<LevelCallback>,
        audio_cb: Option<AudioFrameCallback>,
    ) -> Self {
        Self {
            vad,
            level_cb,
            audio_cb,
            cmd_tx: None,
            quit_tx: None,
            pipewire_handle: None,
            consumer_handle: None,
            healthy: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn open(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.pipewire_handle.is_some() {
            if self.is_healthy() {
                return Ok(());
            }
            self.close()?;
        }

        self.healthy.store(false, Ordering::Release);
        let (sample_tx, sample_rx) = mpsc::channel::<AudioChunk>();
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<u32, String>>(1);
        let (quit_tx, quit_rx) = pw::channel::channel::<()>();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let pipewire_stop_flag = Arc::clone(&stop_flag);
        let pipewire_health = Arc::clone(&self.healthy);
        let stream_started_at = Instant::now();

        let pipewire_handle = std::thread::spawn(move || {
            run_pipewire_loop(
                sample_tx,
                pipewire_stop_flag,
                pipewire_health,
                init_tx,
                quit_rx,
            );
        });

        let sample_rate = match init_rx.recv_timeout(STARTUP_TIMEOUT) {
            Ok(Ok(sample_rate)) => sample_rate,
            Ok(Err(message)) => {
                let _ = quit_tx.send(());
                let _ = pipewire_handle.join();
                return Err(Box::new(Error::other(message)));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = quit_tx.send(());
                let _ = pipewire_handle.join();
                return Err(Box::new(Error::other(
                    "PipeWire capture did not become ready within 5 seconds",
                )));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = pipewire_handle.join();
                return Err(Box::new(Error::other(
                    "PipeWire capture worker exited during startup",
                )));
            }
        };

        let vad = self.vad.clone();
        let level_cb = self.level_cb.clone();
        let audio_cb = self.audio_cb.clone();
        let consumer_stop_flag = Arc::clone(&stop_flag);
        let consumer_handle = std::thread::spawn(move || {
            run_consumer(
                sample_rate,
                vad,
                sample_rx,
                cmd_rx,
                level_cb,
                audio_cb,
                consumer_stop_flag,
                stream_started_at,
            );
        });

        self.cmd_tx = Some(cmd_tx);
        self.quit_tx = Some(quit_tx);
        self.pipewire_handle = Some(pipewire_handle);
        self.consumer_handle = Some(consumer_handle);
        Ok(())
    }

    pub(crate) fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
            && self
                .pipewire_handle
                .as_ref()
                .is_some_and(|handle| !handle.is_finished())
            && self
                .consumer_handle
                .as_ref()
                .is_some_and(|handle| !handle.is_finished())
    }

    pub(crate) fn start(&self, vad_policy: VadPolicy) -> Result<(), Box<dyn std::error::Error>> {
        let tx = self
            .cmd_tx
            .as_ref()
            .ok_or_else(|| Error::other("PipeWire recorder is not open"))?;
        tx.send(Cmd::Start(vad_policy, Instant::now()))?;
        Ok(())
    }

    pub(crate) fn stop(&self) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let tx = self
            .cmd_tx
            .as_ref()
            .ok_or_else(|| Error::other("PipeWire recorder is not open"))?;
        let (response_tx, response_rx) = mpsc::channel();
        tx.send(Cmd::Stop(response_tx))?;
        match response_rx.recv_timeout(STOP_TIMEOUT) {
            Ok(samples) => Ok(samples),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(Box::new(Error::other(
                "Timed out while stopping PipeWire capture",
            ))),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(Box::new(Error::other(
                "PipeWire capture stopped unexpectedly",
            ))),
        }
    }

    pub(crate) fn close(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(Cmd::Shutdown);
        }
        if let Some(tx) = self.quit_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.pipewire_handle.take() {
            handle
                .join()
                .map_err(|_| Error::other("PipeWire capture worker panicked"))?;
        }
        if let Some(handle) = self.consumer_handle.take() {
            handle
                .join()
                .map_err(|_| Error::other("PipeWire audio consumer panicked"))?;
        }
        self.healthy.store(false, Ordering::Release);
        Ok(())
    }
}

impl Drop for PipeWireRecorder {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

struct CaptureState {
    sample_rate: u32,
    channels: u32,
    sample_tx: mpsc::Sender<AudioChunk>,
    stop_flag: Arc<AtomicBool>,
    healthy: Arc<AtomicBool>,
    end_of_stream_sent: bool,
    stream_ready: bool,
    format_ready: bool,
    init_tx: Option<mpsc::SyncSender<Result<u32, String>>>,
}

impl CaptureState {
    fn report_ready_if_possible(&mut self) {
        if !self.stream_ready || !self.format_ready {
            return;
        }
        if let Some(tx) = self.init_tx.take() {
            self.healthy.store(true, Ordering::Release);
            let _ = tx.send(Ok(self.sample_rate));
        }
    }

    fn report_error(&mut self, message: String) {
        self.healthy.store(false, Ordering::Release);
        if let Some(tx) = self.init_tx.take() {
            let _ = tx.send(Err(message));
        } else {
            log::error!("{message}");
        }
    }
}

fn run_pipewire_loop(
    sample_tx: mpsc::Sender<AudioChunk>,
    stop_flag: Arc<AtomicBool>,
    healthy: Arc<AtomicBool>,
    init_tx: mpsc::SyncSender<Result<u32, String>>,
    quit_rx: pw::channel::Receiver<()>,
) {
    let setup = (|| -> Result<(), pw::Error> {
        pw::init();

        let mainloop = pw::MainLoop::new()?;
        let _quit_listener = quit_rx.attach(&mainloop, {
            let mainloop = mainloop.clone();
            move |_| mainloop.quit()
        });

        let props = pw::properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Communication",
            *pw::keys::APP_NAME => APP_NAME,
            *pw::keys::NODE_NAME => NODE_NAME,
        };
        let state_mainloop = mainloop.clone();
        let capture_state = Rc::new(RefCell::new(CaptureState {
            sample_rate: 0,
            channels: 0,
            sample_tx,
            stop_flag,
            healthy: Arc::clone(&healthy),
            end_of_stream_sent: false,
            stream_ready: false,
            format_ready: false,
            init_tx: Some(init_tx.clone()),
        }));

        let stream = pw::stream::Stream::with_user_data(
            &mainloop,
            APP_NAME,
            props,
            Rc::clone(&capture_state),
        )
        .state_changed(move |_old, new| {
            let mut state = capture_state.borrow_mut();
            match new {
                pw::stream::StreamState::Streaming => {
                    state.stream_ready = true;
                    state.report_ready_if_possible();
                }
                pw::stream::StreamState::Paused => {
                    if state.stream_ready {
                        state.report_error(
                            "PipeWire capture stream stopped producing audio".to_string(),
                        );
                        state_mainloop.quit();
                    }
                }
                pw::stream::StreamState::Error(error) => {
                    state.report_error(format!("PipeWire capture stream failed: {error}"));
                    state_mainloop.quit();
                }
                pw::stream::StreamState::Unconnected => {
                    state.report_error(
                        "PipeWire capture stream disconnected unexpectedly".to_string(),
                    );
                    state_mainloop.quit();
                }
                pw::stream::StreamState::Connecting => {}
            }
        })
        .param_changed(|id, state, param| {
            if param.is_null() {
                return;
            }
            if id != libspa_sys::SPA_PARAM_Format {
                return;
            }

            let mut state = state.borrow_mut();
            let mut media_type = 0;
            let mut media_subtype = 0;
            let format_result =
                unsafe { libspa_sys::spa_format_parse(param, &mut media_type, &mut media_subtype) };
            if format_result < 0 {
                state.report_error("PipeWire returned an invalid audio format".to_string());
                return;
            }
            if media_type != libspa_sys::SPA_MEDIA_TYPE_audio
                || media_subtype != libspa_sys::SPA_MEDIA_SUBTYPE_raw
            {
                state.report_error("PipeWire returned a non-raw audio format".to_string());
                return;
            }

            let mut audio_info: libspa_sys::spa_audio_info_raw = unsafe { mem::zeroed() };
            let audio_result =
                unsafe { libspa_sys::spa_format_audio_raw_parse(param, &mut audio_info) };
            if audio_result < 0 {
                state.report_error("PipeWire returned an unreadable raw audio format".to_string());
                return;
            }
            if audio_info.format != libspa_sys::SPA_AUDIO_FORMAT_F32_LE {
                state.report_error(format!(
                    "PipeWire negotiated unsupported sample format {}",
                    audio_info.format
                ));
                return;
            }
            if audio_info.rate == 0 || audio_info.channels == 0 {
                state.report_error(
                    "PipeWire negotiated an invalid sample rate or channel count".to_string(),
                );
                return;
            }

            log::info!(
                "PipeWire capture negotiated: rate={} Hz, channels={}",
                audio_info.rate,
                audio_info.channels
            );
            state.sample_rate = audio_info.rate;
            state.channels = audio_info.channels;
            state.format_ready = true;
            state.report_ready_if_possible();
        })
        .process(|stream, state| {
            let mut state = state.borrow_mut();
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };

            if state.stop_flag.load(Ordering::Relaxed) {
                if !state.end_of_stream_sent {
                    let _ = state.sample_tx.send(AudioChunk::EndOfStream);
                    state.end_of_stream_sent = true;
                }
                return;
            }
            state.end_of_stream_sent = false;

            let channels = state.channels as usize;
            if channels == 0 {
                return;
            }
            let offset = data.chunk().offset() as usize;
            let size = data.chunk().size() as usize;
            let stride = data.chunk().stride();
            let Some(raw) = data.data() else {
                return;
            };

            match downmix_interleaved_f32le(raw, offset, size, stride, channels) {
                Ok(samples) if !samples.is_empty() => {
                    let _ = state.sample_tx.send(AudioChunk::Samples(samples));
                }
                Ok(_) => {}
                Err(error) => log::warn!("Skipping invalid PipeWire audio buffer: {error}"),
            }
        })
        .create()?;

        let format_object = Object {
            type_: libspa_sys::SPA_TYPE_OBJECT_Format,
            id: libspa_sys::SPA_PARAM_EnumFormat,
            properties: vec![
                Property {
                    key: libspa_sys::SPA_FORMAT_mediaType,
                    flags: PropertyFlags::empty(),
                    value: Value::Id(Id(libspa_sys::SPA_MEDIA_TYPE_audio)),
                },
                Property {
                    key: libspa_sys::SPA_FORMAT_mediaSubtype,
                    flags: PropertyFlags::empty(),
                    value: Value::Id(Id(libspa_sys::SPA_MEDIA_SUBTYPE_raw)),
                },
                Property {
                    key: libspa_sys::SPA_FORMAT_AUDIO_format,
                    flags: PropertyFlags::empty(),
                    value: Value::Id(Id(libspa_sys::SPA_AUDIO_FORMAT_F32_LE)),
                },
                Property {
                    key: libspa_sys::SPA_FORMAT_AUDIO_rate,
                    flags: PropertyFlags::empty(),
                    value: Value::Int(PREFERRED_SAMPLE_RATE as i32),
                },
            ],
        };
        let values =
            PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(format_object))
                .map_err(|_| pw::Error::CreationFailed)?
                .0
                .into_inner();
        let mut params = [values.as_ptr().cast::<libspa_sys::spa_pod>()];

        stream.connect(
            Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )?;

        mainloop.run();
        Ok(())
    })();

    if let Err(error) = setup {
        let _ = init_tx.send(Err(format!("PipeWire capture setup failed: {error}")));
    }
    healthy.store(false, Ordering::Release);
}

fn downmix_interleaved_f32le(
    data: &[u8],
    offset: usize,
    size: usize,
    stride: i32,
    channels: usize,
) -> Result<Vec<f32>, &'static str> {
    if channels == 0 {
        return Err("zero channels");
    }
    let sample_bytes = mem::size_of::<f32>();
    let frame_bytes = channels
        .checked_mul(sample_bytes)
        .ok_or("channel count overflow")?;
    let frame_stride = if stride > 0 {
        stride as usize
    } else {
        frame_bytes
    };
    if frame_stride < frame_bytes {
        return Err("frame stride is smaller than one audio frame");
    }
    let end = offset.checked_add(size).ok_or("buffer range overflow")?;
    if end > data.len() {
        return Err("buffer range exceeds mapped data");
    }

    let frame_count = size / frame_stride;
    let mut mono = Vec::with_capacity(frame_count);
    for frame in 0..frame_count {
        let frame_start = offset + frame * frame_stride;
        let mut sum = 0.0f32;
        for channel in 0..channels {
            let start = frame_start + channel * sample_bytes;
            let bytes: [u8; 4] = data[start..start + sample_bytes]
                .try_into()
                .map_err(|_| "incomplete sample")?;
            sum += f32::from_le_bytes(bytes);
        }
        mono.push(sum / channels as f32);
    }
    Ok(mono)
}

#[cfg(test)]
mod tests {
    use super::downmix_interleaved_f32le;

    fn samples_to_bytes(samples: &[f32]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    #[test]
    fn downmixes_stereo_frames() {
        let bytes = samples_to_bytes(&[0.5, -0.5, 0.25, 0.75]);
        assert_eq!(
            downmix_interleaved_f32le(&bytes, 0, bytes.len(), 8, 2).unwrap(),
            vec![0.0, 0.5]
        );
    }

    #[test]
    fn honors_offset_and_padded_stride() {
        let mut bytes = vec![9, 9, 9, 9];
        bytes.extend(samples_to_bytes(&[0.25, 0.75]));
        bytes.extend([0, 0, 0, 0]);
        bytes.extend(samples_to_bytes(&[-0.5, 0.5]));
        bytes.extend([0, 0, 0, 0]);

        assert_eq!(
            downmix_interleaved_f32le(&bytes, 4, 24, 12, 2).unwrap(),
            vec![0.5, 0.0]
        );
    }

    #[test]
    fn rejects_out_of_bounds_buffers() {
        let bytes = samples_to_bytes(&[0.5]);
        assert!(downmix_interleaved_f32le(&bytes, 4, 4, 4, 1).is_err());
    }
}
