use super::*;

struct StalledControl {
    entered: mpsc::Sender<()>,
    release: Receiver<()>,
    bitrate: u32,
    forced: bool,
}

impl Encoder<u8> for StalledControl {
    type Output = (u8, bool, u32);
    type Error = &'static str;
    fn force_keyframe(&mut self) {
        self.forced = true;
    }
    fn bitrate(&mut self, bitrate: u32) -> Result<Vec<Self::Output>, Self::Error> {
        if bitrate == 8 {
            self.entered.send(()).unwrap();
            self.release
                .recv_timeout(BOUND)
                .map_err(|_| "release timeout")?;
        }
        self.bitrate = bitrate;
        Ok(vec![(99, false, bitrate)])
    }
    fn encode(&mut self, frame: u8) -> Result<Vec<Self::Output>, Self::Error> {
        Ok(vec![(frame, self.forced, self.bitrate)])
    }
}

#[test]
fn reselects_after_bitrate_stall() {
    // Given A selected and a subscribed, channel-gated bitrate operation.
    let handoff = Arc::new(Handoff::new());
    handoff.publish(1);
    handoff.control(true, Some(8));
    let (entered, entry) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let worker_handoff = Arc::clone(&handoff);
    let join = thread::spawn(move || {
        let mut outputs = Vec::new();
        let result = run_encoder(
            &worker_handoff,
            || {
                Ok(StalledControl {
                    entered,
                    release: gate,
                    bitrate: 0,
                    forced: false,
                })
            },
            |packet| {
                outputs.push(packet);
                packet.0 == 99
            },
        );
        (result, outputs)
    });
    let observed = entry.recv_timeout(BOUND);
    // When newer raw work and controls arrive during the stall.
    handoff.publish(2);
    handoff.control(false, Some(4));
    handoff.publish(3);
    handoff.control(true, Some(2));
    let released = release.send(());
    let (result, outputs) = join.join().unwrap();
    // Then all drained compressed outputs precede only C, with newest controls.
    assert_eq!(observed, Ok(()));
    assert!(released.is_ok());
    assert_eq!(result, Ok(()));
    assert_eq!(outputs, [(99, false, 8), (99, false, 2), (3, true, 2)]);
}
