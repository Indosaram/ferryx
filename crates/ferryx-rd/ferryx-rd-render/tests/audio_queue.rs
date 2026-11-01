use ferryx_rd_render::{AudioQueue, AUDIO_CHANNELS, AUDIO_SAMPLE_RATE};

const CAPACITY: usize = AUDIO_SAMPLE_RATE as usize * AUDIO_CHANNELS as usize / 10;

fn frames(count: usize) -> Vec<f32> {
    (0..count)
        .flat_map(|i| [i as f32 / 10000.0, -(i as f32 + 1.0) / 10000.0])
        .collect()
}

fn assert_queue(queue: &AudioQueue, expected: &[f32]) {
    assert_eq!(queue.queued_samples().unwrap(), expected.len());
    let mut actual = vec![123.0; expected.len()];
    assert_eq!(queue.drain_into(&mut actual).unwrap(), expected.len());
    assert_eq!(actual, expected);
}

#[test]
fn overflow_retains_newest_complete_stereo_frames() {
    let queue = AudioQueue::default();
    let input = frames(CAPACITY / 2 + 5);
    queue.push_samples(input.iter().copied()).unwrap();
    assert_queue(&queue, &input[10..]);
    queue
        .push_samples(input[..CAPACITY].iter().copied())
        .unwrap();
    queue.push_samples([0.125, -0.25]).unwrap();
    let mut expected = input[2..CAPACITY].to_vec();
    expected.extend([0.125, -0.25]);
    assert_queue(&queue, &expected);
}

#[test]
fn pcm_bytes_overflow_retains_newest_frames() {
    let queue = AudioQueue::default();
    let input = frames(CAPACITY / 2 + 5);
    let bytes: Vec<_> = input.iter().flat_map(|value| value.to_le_bytes()).collect();
    queue.push_pcm_bytes(&bytes).unwrap();
    assert_queue(&queue, &input[10..]);
}

#[test]
fn invalid_samples_are_rejected_atomically() {
    for invalid in [
        vec![0.5],
        vec![0.5, f32::NAN],
        vec![f32::INFINITY, 0.5],
        vec![0.5, f32::NEG_INFINITY],
        {
            let mut values = vec![f32::NAN, 0.0];
            values.extend(frames(CAPACITY));
            values
        },
    ] {
        let queue = AudioQueue::default();
        queue.push_samples([0.125, -0.25]).unwrap();
        assert!(queue.push_samples(invalid).is_err());
        assert_queue(&queue, &[0.125, -0.25]);
    }
}

#[test]
fn invalid_pcm_bytes_are_rejected_atomically() {
    for invalid in [
        vec![0],
        0.5f32.to_le_bytes().to_vec(),
        [0.5f32, f32::NAN]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect(),
        [f32::INFINITY, 0.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect(),
    ] {
        let queue = AudioQueue::default();
        queue.push_samples([0.125, -0.25]).unwrap();
        assert!(queue.push_pcm_bytes(&invalid).is_err());
        assert_queue(&queue, &[0.125, -0.25]);
    }
}

#[test]
fn invalid_volume_is_rejected_without_changing_gain() {
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.01, 1.01] {
        let queue = AudioQueue::default();
        queue.set_volume(0.5).unwrap();
        assert!(queue.set_volume(invalid).is_err());
        queue.push_samples([0.25, -0.5]).unwrap();
        assert_queue(&queue, &[0.125, -0.25]);
    }
}

#[test]
fn misaligned_drain_does_not_split_stereo_frames() {
    let queue = AudioQueue::default();
    queue.push_samples([0.125, -0.25]).unwrap();
    let mut output = [123.0];
    assert!(queue.drain_into(&mut output).is_err());
    assert_eq!(output, [123.0]);
    assert_queue(&queue, &[0.125, -0.25]);
}

#[test]
fn underrun_zero_fills_and_mute_consumes() {
    let queue = AudioQueue::default();
    queue.set_volume(0.5).unwrap();
    queue.push_samples([0.25, -0.5]).unwrap();
    let mut output = [123.0; 6];
    assert_eq!(queue.drain_into(&mut output).unwrap(), 2);
    assert_eq!(output, [0.125, -0.25, 0.0, 0.0, 0.0, 0.0]);
    queue.set_muted(true).unwrap();
    queue.push_samples([0.25, -0.5]).unwrap();
    output.fill(123.0);
    assert_eq!(queue.drain_into(&mut output).unwrap(), 2);
    assert_eq!(output, [0.0; 6]);
    assert_eq!(queue.queued_samples().unwrap(), 0);
    queue.set_muted(false).unwrap();
    assert_eq!(queue.drain_into(&mut output).unwrap(), 0);
}

#[test]
fn little_endian_pcm_preserves_channel_values() {
    let queue = AudioQueue::default();
    let input = [0.125f32, -0.25, 0.75, -0.5];
    let bytes: Vec<_> = input.into_iter().flat_map(f32::to_le_bytes).collect();
    queue.push_pcm_bytes(&bytes).unwrap();
    queue.push_pcm_bytes(&[]).unwrap();
    queue.push_samples([]).unwrap();
    assert_queue(&queue, &input);
}

#[test]
fn clear_is_shared_and_preserves_gain_and_mute() {
    let queue = AudioQueue::default();
    let producer = queue.clone();
    queue.set_volume(0.5).unwrap();
    queue.set_muted(true).unwrap();
    producer.push_samples([0.125, -0.25]).unwrap();
    queue.clear().unwrap();
    assert_eq!(producer.queued_samples().unwrap(), 0);
    producer.push_samples([0.25, -0.5]).unwrap();
    assert_queue(&queue, &[0.0, 0.0]);
    queue.set_muted(false).unwrap();
    producer.push_samples([0.25, -0.5]).unwrap();
    assert_queue(&queue, &[0.125, -0.25]);
    queue.clear().unwrap();
    queue.clear().unwrap();
}

#[test]
fn volume_endpoints_are_valid() {
    let queue = AudioQueue::default();
    for gain in [0.0, 1.0] {
        queue.set_volume(gain).unwrap();
        queue.push_samples([0.25, -0.5]).unwrap();
        assert_queue(&queue, &[0.25 * gain, -0.5 * gain]);
    }
}
