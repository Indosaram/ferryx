use ferryx_rd_render::{
    AudioError, AudioOutputEvent, AudioOutputStatus, AudioQueue, AUDIO_CHANNELS,
    AUDIO_QUEUE_CAPACITY, AUDIO_SAMPLE_RATE,
};

#[test]
fn pure_public_api_preserves_format_controls_and_notifications() {
    assert_eq!(AUDIO_SAMPLE_RATE, 48_000);
    assert_eq!(AUDIO_CHANNELS, 2);
    assert_eq!(AUDIO_QUEUE_CAPACITY, 9_600);
    let queue = AudioQueue::default();
    assert!(matches!(
        queue.push_pcm_bytes(&[0; 4]),
        Err(AudioError::MisalignedPcm(4))
    ));
    assert!(matches!(
        queue.push_samples([0.0]),
        Err(AudioError::MisalignedSamples)
    ));
    assert!(matches!(
        queue.push_samples([f32::NAN, 0.0]),
        Err(AudioError::NonFinitePcm)
    ));
    assert!(matches!(
        queue.set_volume(2.0),
        Err(AudioError::InvalidVolume(2.0))
    ));
    queue.set_volume(0.5).unwrap();
    queue.set_muted(false).unwrap();
    queue.push_samples([0.5, -0.25]).unwrap();
    let mut output = [123.0; 4];
    let consumed = queue.drain_into(&mut output).unwrap();
    assert_eq!(output, [0.25, -0.125, 0.0, 0.0]);

    // These transport-neutral payloads remain usable by a caller-owned sink.
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    sender
        .try_send(AudioOutputEvent::Callback {
            consumed_samples: consumed,
            total_consumed_samples: consumed as u64,
        })
        .unwrap();
    let mut status = AudioOutputStatus::default();
    match receiver.try_recv().unwrap() {
        AudioOutputEvent::Callback {
            consumed_samples,
            total_consumed_samples,
        } => {
            assert_eq!(consumed_samples, 2);
            status.callback_count += 1;
            status.consumed_samples = total_consumed_samples;
        }
        AudioOutputEvent::Error(error) => panic!("unexpected error: {error}"),
    }
    assert_eq!(status.callback_count, 1);
    assert_eq!(status.consumed_samples, 2);
    assert_eq!(status.error_count, 0);
    assert_eq!(status.last_error, None);
}

#[cfg(feature = "cpal-output")]
#[test]
fn native_public_api_retains_cpal_types_and_entry_points() {
    use ferryx_rd_render::{AudioOutputDevice, CpalAudioOutput};
    use std::sync::mpsc::SyncSender;

    // Type-check the desktop contract without opening a physical output device.
    let _: fn() -> Result<Vec<AudioOutputDevice>, AudioError> = CpalAudioOutput::output_devices;
    let _: fn() -> Result<AudioOutputDevice, AudioError> = CpalAudioOutput::default_output_device;
    let _: fn(AudioQueue) -> Result<CpalAudioOutput, AudioError> = CpalAudioOutput::start;
    let _: fn(AudioQueue, Option<&AudioOutputDevice>) -> Result<CpalAudioOutput, AudioError> =
        CpalAudioOutput::start_on_device;
    type StartWithEventsFn = fn(
        AudioQueue,
        Option<&AudioOutputDevice>,
        SyncSender<AudioOutputEvent>,
    ) -> Result<CpalAudioOutput, AudioError>;
    let _: StartWithEventsFn = CpalAudioOutput::start_with_events;
    let _: fn(&AudioOutputDevice) -> Result<String, AudioError> = AudioOutputDevice::name;
    let _: fn(&AudioOutputDevice) -> Result<bool, AudioError> = AudioOutputDevice::supports_pcm;
    let _: fn(&CpalAudioOutput) -> &AudioQueue = CpalAudioOutput::queue;
    let _: fn(&CpalAudioOutput) -> &cpal::Stream = CpalAudioOutput::stream;
    let _: fn(&CpalAudioOutput) -> Result<AudioOutputStatus, AudioError> = CpalAudioOutput::status;
    let _: fn(&CpalAudioOutput) -> Result<(), AudioError> = CpalAudioOutput::clear;
    // Exercise the real error value carried across the public API: the variant
    // must retain its cpal payload and render the sample format through Display.
    let error = AudioError::UnsupportedSampleFormat(cpal::SampleFormat::I16);
    assert!(format!("{error}").contains("I16"));
    let error = AudioError::UnsupportedSampleFormat(cpal::SampleFormat::F32);
    assert!(!matches!(
        error,
        AudioError::UnsupportedSampleFormat(cpal::SampleFormat::I16)
    ));
}
