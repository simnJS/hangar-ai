//! Microphone capture, resampled on the way in to what speech models want.
//!
//! The cpal stream lives on a thread of its own and never leaves it. That is
//! not a style choice: a stream is not `Send` on every backend, so it cannot be
//! parked in the manager's state next to everything else, and its callback runs
//! on the audio thread where blocking on a lock held by a slow reader would be
//! heard as a glitch. The thread owns the stream, the callback appends to a
//! buffer, and the outside world sees a handle it can stop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};

/// Every model here — Parakeet, Whisper, and the cloud APIs behind them alike —
/// is trained on 16 kHz mono. Anything else is resampled before it is seen.
pub const SAMPLE_RATE: u32 = 16_000;

/// A dictation that runs this long was left recording by accident. Capture
/// stops on its own rather than growing a buffer until the machine notices:
/// five minutes is far past any single spoken instruction, and costs 19 MB.
const MAX_SECONDS: usize = 300;
const MAX_SAMPLES: usize = MAX_SECONDS * SAMPLE_RATE as usize;

/// How long `start` waits for the device to open before calling it a failure.
/// Opening is near instant; this only bounds the wait when the audio backend
/// itself is wedged, so a broken microphone fails the dictation rather than
/// hanging the app.
const OPEN_TIMEOUT: Duration = Duration::from_secs(3);

/// How often the capture thread wakes to check whether it should stop. Also the
/// worst case delay between releasing the key and the recording ending, which
/// is why it is short enough not to clip the last syllable.
const POLL: Duration = Duration::from_millis(20);

#[derive(Default)]
struct Shared {
    samples: Mutex<Vec<f32>>,
    stop: AtomicBool,
    /// Loudest sample seen since the meter last read it. Peak rather than RMS:
    /// the meter is there to prove the microphone is live, and a peak answers
    /// that within one syllable where an average takes a breath to move.
    peak: Mutex<f32>,
}

pub struct Recorder {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Recorder {
    /// Opens the default input device and starts filling the buffer.
    ///
    /// Returns once the device is confirmed open, so a machine with no
    /// microphone — or one whose permission was refused — fails here, while the
    /// user still has the key held down and an error can mean something.
    pub fn start() -> Result<Self, String> {
        let shared = Arc::new(Shared::default());
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

        let thread_shared = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("voice-capture".into())
            .spawn(move || run(thread_shared, ready_tx))
            .map_err(|err| format!("could not start capture thread: {err}"))?;

        match ready_rx.recv_timeout(OPEN_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                shared,
                thread: Some(thread),
            }),
            Ok(Err(err)) => Err(err),
            // The thread died before answering, or the audio backend never came
            // back. Either way there is no stream, and nothing to stop.
            Err(_) => Err("the microphone did not open".into()),
        }
    }

    /// Loudest sample since the last call, as 0..1. Reading clears it, so each
    /// frame of the meter describes its own slice of time.
    pub fn level(&self) -> f32 {
        let mut peak = self.shared.peak.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *peak)
    }

    /// Ends the capture and hands back everything recorded, at 16 kHz mono.
    ///
    /// Joins the thread first: the callback holds the same buffer, and taking it
    /// while the stream is still open would drop whatever was spoken in the last
    /// few milliseconds — which is exactly the end of the sentence.
    pub fn finish(mut self) -> Vec<f32> {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        std::mem::take(&mut *self.shared.samples.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl Drop for Recorder {
    /// A recorder dropped without `finish` — a dictation cancelled, a panic on
    /// the way — still has a thread holding the microphone open. Windows shows
    /// that as a live recording indicator, so it is worth ending properly.
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(shared: Arc<Shared>, ready: mpsc::Sender<Result<(), String>>) {
    let stream = match open(&shared) {
        Ok(stream) => {
            // Reported before `play`, not after: the send cannot fail in a way
            // worth reacting to — a caller that timed out has already given up.
            let _ = ready.send(Ok(()));
            stream
        }
        Err(err) => {
            let _ = ready.send(Err(err));
            return;
        }
    };

    if let Err(err) = stream.play() {
        let _ = ready.send(Err(err.to_string()));
        return;
    }

    while !shared.stop.load(Ordering::Relaxed) {
        // The cap is checked here rather than in the callback so the audio
        // thread does nothing but copy samples.
        let full = shared
            .samples
            .lock()
            .map(|s| s.len() >= MAX_SAMPLES)
            .unwrap_or(true);
        if full {
            break;
        }
        thread::sleep(POLL);
    }

    // Dropping the stream is what closes the device and releases the recording
    // indicator; doing it here rather than at the end of the scope makes that
    // the last thing the thread does before the join returns.
    drop(stream);
}

fn open(shared: &Arc<Shared>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("no microphone was found on this system")?;
    let config = device
        .default_input_config()
        .map_err(|err| format!("the microphone refused its own default format: {err}"))?;

    let format = config.sample_format();
    let channels = config.channels() as usize;
    let rate = config.sample_rate();
    let stream_config: cpal::StreamConfig = config.into();

    // The device's own default is used as it is rather than asking for 16 kHz
    // mono: a driver that cannot deliver exactly that fails the whole open,
    // where resampling always works. Shared devices — a headset already on a
    // call — are the common case, and they rarely offer a choice.
    match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, stream_config, shared, channels, rate),
        cpal::SampleFormat::I16 => build::<i16>(&device, stream_config, shared, channels, rate),
        cpal::SampleFormat::U16 => build::<u16>(&device, stream_config, shared, channels, rate),
        cpal::SampleFormat::I32 => build::<i32>(&device, stream_config, shared, channels, rate),
        cpal::SampleFormat::I8 => build::<i8>(&device, stream_config, shared, channels, rate),
        cpal::SampleFormat::U8 => build::<u8>(&device, stream_config, shared, channels, rate),
        other => Err(format!("unsupported microphone sample format: {other}")),
    }
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    shared: &Arc<Shared>,
    channels: usize,
    rate: u32,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let shared = Arc::clone(shared);
    let mut resampler = Resampler::new(rate, SAMPLE_RATE);
    // Reused across callbacks so the audio thread allocates nothing per block.
    let mut converted: Vec<f32> = Vec::with_capacity(1024);

    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                converted.clear();
                let mut peak = 0.0f32;

                for frame in data.chunks(channels.max(1)) {
                    // Channels are averaged rather than picking the first: a
                    // laptop's second microphone carries as much of the voice
                    // as its first, and averaging is what cancels the room.
                    let mut sum = 0.0f32;
                    for sample in frame {
                        sum += f32::from_sample(*sample);
                    }
                    let mono = sum / frame.len() as f32;
                    peak = peak.max(mono.abs());
                    resampler.push(mono, &mut converted);
                }

                if let Ok(mut samples) = shared.samples.lock() {
                    samples.extend_from_slice(&converted);
                }
                if let Ok(mut level) = shared.peak.lock() {
                    *level = level.max(peak);
                }
            },
            // An error here is the device disappearing mid-sentence — a headset
            // unplugged. The stream is dead either way; the dictation ends with
            // whatever was captured before it, which is the kindest outcome.
            move |err| eprintln!("microphone stream error: {err}"),
            None,
        )
        .map_err(|err| format!("could not open the microphone: {err}"))
}

/// Rate conversion by box averaging.
///
/// Each output sample is the mean of the input samples that fall inside its
/// window, which is a crude low-pass filter — and crude is the right amount
/// here. Speech carries almost nothing above 8 kHz, the models were trained on
/// material resampled much the same way, and this costs one add per sample on
/// the audio thread where a proper polyphase filter would cost a dependency and
/// a buffer. The window is tracked in floating point, so 44.1 kHz converts as
/// exactly as the integer ratios do.
struct Resampler {
    /// Input samples per output sample.
    ratio: f64,
    /// How much of the current window is still unfilled.
    remaining: f64,
    sum: f32,
    count: f32,
}

impl Resampler {
    fn new(from: u32, to: u32) -> Self {
        let ratio = (from as f64 / to as f64).max(f64::MIN_POSITIVE);
        Self {
            ratio,
            remaining: ratio,
            sum: 0.0,
            count: 0.0,
        }
    }

    fn push(&mut self, sample: f32, out: &mut Vec<f32>) {
        self.sum += sample;
        self.count += 1.0;
        self.remaining -= 1.0;

        // A loop rather than an `if`: upsampling from a device that only offers
        // 8 kHz needs several output samples out of one input sample, and the
        // window can be short enough to hold nothing of its own.
        while self.remaining <= 0.0 {
            out.push(if self.count > 0.0 {
                self.sum / self.count
            } else {
                sample
            });
            self.sum = 0.0;
            self.count = 0.0;
            self.remaining += self.ratio;
        }
    }
}

/// Root mean square of a slice, as a rough "was anything said" measure.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// Silence at the head and tail, trimmed with a margin.
///
/// Push-to-talk always records some: the moment between pressing the key and
/// starting to speak, and the one between finishing and letting go. Whisper in
/// particular answers leading silence with an invented sentence — the famous
/// "Thank you." on an empty clip — so this is accuracy work, not just a way to
/// send fewer bytes.
pub fn trim(samples: &[f32], threshold: f32, margin_ms: usize) -> &[f32] {
    let window = (SAMPLE_RATE as usize / 100).max(1); // 10 ms
    let margin = margin_ms * SAMPLE_RATE as usize / 1000;

    let loud = |chunk: &[f32]| rms(chunk) > threshold;

    let first = samples.chunks(window).position(|c| loud(c));
    let Some(first) = first else {
        return &[];
    };
    let last = samples
        .chunks(window)
        .rposition(|c| loud(c))
        .unwrap_or(first);

    let start = (first * window).saturating_sub(margin);
    let end = ((last + 1) * window + margin).min(samples.len());
    &samples[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a whole buffer through the resampler, as the callback would.
    fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
        let mut resampler = Resampler::new(from, to);
        let mut out = Vec::new();
        for sample in input {
            resampler.push(*sample, &mut out);
        }
        out
    }

    #[test]
    fn an_integer_ratio_decimates_exactly() {
        let input: Vec<f32> = (0..48_000).map(|_| 0.5).collect();
        let out = resample(&input, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
        // A constant survives averaging, whatever the window lands on.
        assert!(out.iter().all(|s| (*s - 0.5).abs() < 1e-6));
    }

    /// 44.1 kHz does not divide into 16 kHz, and the fractional window is the
    /// whole reason this is tracked in floating point: an integer counter
    /// drifts by a sample every few hundred, which over a minute of dictation
    /// is a second of audio invented or lost.
    #[test]
    fn a_fractional_ratio_keeps_its_length() {
        let input: Vec<f32> = (0..44_100).map(|_| 0.25).collect();
        let out = resample(&input, 44_100, 16_000);
        assert!(
            (out.len() as i64 - 16_000).abs() <= 1,
            "expected about 16000 samples, got {}",
            out.len()
        );
    }

    #[test]
    fn upsampling_fills_rather_than_stalls() {
        let input: Vec<f32> = (0..8_000).map(|_| 0.1).collect();
        let out = resample(&input, 8_000, 16_000);
        assert!((out.len() as i64 - 16_000).abs() <= 1);
        assert!(out.iter().all(|s| (*s - 0.1).abs() < 1e-6));
    }

    #[test]
    fn silence_trims_to_nothing() {
        let quiet = vec![0.0f32; SAMPLE_RATE as usize];
        assert!(trim(&quiet, 0.01, 100).is_empty());
    }

    #[test]
    fn speech_keeps_its_edges() {
        let rate = SAMPLE_RATE as usize;
        let mut samples = vec![0.0f32; rate];
        // Half a second of tone in the middle of a second of silence.
        for sample in samples.iter_mut().take(rate * 3 / 4).skip(rate / 4) {
            *sample = 0.5;
        }

        let kept = trim(&samples, 0.01, 100);
        // The margin either side is kept, so this is longer than the speech —
        // and still far shorter than what went in.
        assert!(kept.len() > rate / 2, "clipped the speech: {}", kept.len());
        assert!(kept.len() < rate, "trimmed nothing: {}", kept.len());
    }

    #[test]
    fn rms_of_silence_is_zero() {
        assert_eq!(rms(&[0.0; 128]), 0.0);
        assert!(rms(&[]) == 0.0);
    }
}
