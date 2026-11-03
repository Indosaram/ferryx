use std::collections::VecDeque;

/// NTP minimum-network-RTT clock correlation. Offset is host minus client.
#[derive(Default)]
pub(crate) struct ClockCorrelation {
    samples: VecDeque<(u64, i128)>,
}

impl ClockCorrelation {
    pub(crate) fn observe(&mut self, probe: ferryx_rd_proto::ClockProbe, received_us: u64) {
        let Some(round_trip) = received_us.checked_sub(probe.client_send_us) else {
            return;
        };
        let Some(processing) = probe.host_send_us.checked_sub(probe.host_receive_us) else {
            return;
        };
        let Some(network) = round_trip.checked_sub(processing) else {
            return;
        };
        let offset = ((i128::from(probe.host_receive_us) - i128::from(probe.client_send_us))
            + (i128::from(probe.host_send_us) - i128::from(received_us)))
            / 2;
        if self.samples.len() == 32 {
            self.samples.pop_front();
        }
        self.samples.push_back((network, offset));
    }

    pub(crate) fn client_time(&self, host_us: u64) -> Option<u64> {
        let (_, offset) = self.samples.iter().min_by_key(|(rtt, _)| rtt)?;
        u64::try_from(i128::from(host_us) - offset).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minimum_rtt_rejects_queue_delay_and_accounts_for_processing() {
        let mut clock = ClockCorrelation::default();
        assert_eq!(clock.client_time(5000), None);
        clock.observe(
            ferryx_rd_proto::ClockProbe {
                client_send_us: 100,
                host_receive_us: 1200,
                host_send_us: 1250,
            },
            350,
        );
        assert_eq!(clock.client_time(1300), Some(300));
        clock.observe(
            ferryx_rd_proto::ClockProbe {
                client_send_us: 1000,
                host_receive_us: 2400,
                host_send_us: 2450,
            },
            1550,
        );
        assert_eq!(clock.client_time(1300), Some(300));
    }
    #[test]
    fn malformed_samples_do_not_create_an_offset() {
        let mut clock = ClockCorrelation::default();
        clock.observe(
            ferryx_rd_proto::ClockProbe {
                client_send_us: 10,
                host_receive_us: 30,
                host_send_us: 20,
            },
            20,
        );
        assert_eq!(clock.client_time(30), None);
    }
}
