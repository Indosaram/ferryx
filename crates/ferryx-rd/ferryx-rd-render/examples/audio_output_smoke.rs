//! Real-device callback smoke, not an audible-fidelity test.
//! Run with --default (also the no-argument default) or --device-index N.
use std::{
    error::Error,
    io,
    sync::mpsc::{sync_channel, TryRecvError},
    time::{Duration, Instant},
};

use cpal::traits::StreamTrait;
use ferryx_rd_render::{
    AudioOutputEvent, AudioQueue, CpalAudioOutput, AUDIO_QUEUE_CAPACITY, AUDIO_SAMPLE_RATE,
};

fn main() -> Result<(), Box<dyn Error>> {
    let devices = CpalAudioOutput::output_devices()?;
    for (index, device) in devices.iter().enumerate() {
        println!(
            "DEVICE index={index} name={:?} supports_pcm={:?}",
            device.name(),
            device.supports_pcm()
        );
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    let selected = match args.as_slice() {
        [] => None,
        [flag] if flag == "--default" => None,
        [flag, index] if flag == "--device-index" => {
            let index: usize = index.parse()?;
            Some(
                devices
                    .get(index)
                    .ok_or_else(|| io::Error::other("device index out of range"))?,
            )
        }
        _ => {
            return Err(io::Error::other(
                "usage: audio_output_smoke [--default | --device-index N]",
            )
            .into())
        }
    };
    let name = match selected {
        Some(device) => device.name()?,
        None => CpalAudioOutput::default_output_device()?.name()?,
    };
    println!(
        "SELECT mode={} name={name:?}",
        if selected.is_some() {
            "explicit"
        } else {
            "default"
        }
    );

    let queue = AudioQueue::default();
    // Subscribe before enqueue/start. Notifications carry cumulative consumption,
    // so a bounded channel dropping progress events cannot lose the final state.
    let (sender, receiver) = sync_channel(32);
    queue.push_samples((0..AUDIO_QUEUE_CAPACITY / 2).flat_map(|frame| {
        let time = frame as f32 / AUDIO_SAMPLE_RATE as f32;
        [440.0f32, 660.0].map(|hz| 0.002 * (std::f32::consts::TAU * hz * time).sin())
    }))?;
    println!(
        "SIGNAL frames=4800 samples={} gain=0.002 left_hz=440 right_hz=660",
        AUDIO_QUEUE_CAPACITY
    );
    let output = match CpalAudioOutput::start_with_events(queue.clone(), selected, sender) {
        Ok(output) => output,
        Err(error) => {
            queue.clear()?;
            println!(
                "START_FAILED error={error} cleanup_queued_samples={}",
                queue.queued_samples()?
            );
            return Err(error.into());
        }
    };
    let scenario = (|| -> Result<(), Box<dyn Error>> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        "callback consumption deadline expired",
                    )
                })?;
            match receiver.recv_timeout(remaining)? {
                AudioOutputEvent::Callback {
                    consumed_samples,
                    total_consumed_samples,
                } => {
                    if total_consumed_samples >= AUDIO_QUEUE_CAPACITY as u64 {
                        println!("CALLBACK delivered_samples={total_consumed_samples} last_consumed={consumed_samples}");
                        break;
                    }
                }
                AudioOutputEvent::Error(error) => return Err(io::Error::other(error).into()),
            }
        }
        let status = output.status()?;
        println!("STATUS {status:?}");
        if status.error_count != 0 {
            return Err(
                io::Error::other(format!("callback errors: {:?}", status.last_error)).into(),
            );
        }
        // Pause synchronously before putting a tail in the queue, proving Drop
        // actually clears pending audio rather than merely finding an empty queue.
        output.stream().pause()?;
        queue.push_samples([0.001, -0.001])?;
        println!("BEFORE_DROP queued_samples={}", queue.queued_samples()?);
        Ok(())
    })();
    drop(output); // Also runs on a callback timeout/error, before returning failure.
    let remaining = queue.queued_samples()?;
    let pending_events = receiver.try_iter().count();
    let callbacks_released = receiver.try_recv() == Err(TryRecvError::Disconnected);
    println!("CLEANUP output_dropped=true queued_samples={remaining} callbacks_released={callbacks_released} pending_events={pending_events}");
    if remaining != 0 || !callbacks_released {
        return Err(
            io::Error::other("output cleanup did not release queue/callback ownership").into(),
        );
    }
    scenario?;
    println!("PASS real CPAL callback delivery; audible fidelity not assessed");
    Ok(())
}
