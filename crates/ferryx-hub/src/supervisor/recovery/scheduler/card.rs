//! Owner answers to recovery cards (M3): every option of a card does what its wording says.
use super::*;

impl RecoveryScheduler {
    /// The owner tapped option `option_index` of the recovery card `prompt_id`.
    ///
    /// - keep going: the card closes, the incident gets a fresh retry budget and restart count;
    /// - stop: every member still in the fleet is interrupted once (a failure keeps the card
    ///   open and a delivered Ctrl-C is never repeated), the incident closes with a `Stopped`
    ///   row and the sessions are left alone from then on;
    /// - leave: the incident closes and the sessions are left alone.
    ///
    /// This is the owner's own action, so it does not wait for the pause flag.
    pub async fn answer_card(&self, prompt_id: &str, option_index: usize) -> Result<(), CardError> {
        let now = self.deps.clock.now_ms();
        let key = prompt_id.strip_prefix(CARD_PREFIX).ok_or(CardError::NotOpen)?;
        let mut st = self.state.lock().await;
        let open_at = st
            .incidents
            .iter()
            .position(|i| i.key == key && i.escalated.is_some());
        let open_refused = st
            .refused
            .iter()
            .find(|(_, i)| i.key == key && i.escalated.is_some())
            .map(|(r, _)| r.clone());
        let (mut inc, refused) = match (open_at, open_refused) {
            (Some(at), _) => (st.incidents.remove(at), false),
            (None, Some(r)) => match st.refused.remove(&r) {
                Some(inc) => (inc, true),
                None => return Err(CardError::NotOpen),
            },
            (None, None) => return Err(CardError::NotOpen),
        };
        let Some(choice) = inc.choices.get(option_index).copied() else {
            self.put_back(&mut st, inc, refused);
            return Err(CardError::BadOption);
        };
        match choice {
            Choice::KeepGoing => {
                if let Some(session) = inc.escalated.take() {
                    self.deps.escalations.resolve(&session);
                }
                inc.budget_from_ms = now;
                inc.attempts = 0;
                inc.restarts = 0;
                if inc.fault != Fault::Quota {
                    inc.next_at_ms = now;
                }
                let summary = format!("Keeping on with {} at the owner's request", inc.title);
                self.row(&mut inc, ActionKind::Retried, summary, true, now).await;
                st.incidents.push(inc);
            }
            Choice::Stop => {
                let mut members: Vec<SessionRef> = inc
                    .ever
                    .iter()
                    .filter(|r| st.present.contains(*r) && !inc.stopped.contains(*r))
                    .cloned()
                    .collect();
                members.sort_by(|a, b| a.as_str().cmp(b.as_str()));
                for r in members {
                    if let Err(error) = self.deps.control.stop(&r).await {
                        self.put_back(&mut st, inc, refused);
                        return Err(CardError::Failed(error.to_string()));
                    }
                    inc.stopped.insert(r.clone());
                    st.sent.remove(&r);
                }
                let n = inc.stopped.len();
                let summary = format!("Stopped {} at the owner's request", sessions(n));
                self.row(&mut inc, ActionKind::Stopped, summary, false, now).await;
                st.dismissed.extend(inc.ever.iter().cloned());
                self.finish(&mut inc);
                self.settle(&mut st, inc, refused);
            }
            Choice::Leave => {
                let summary = format!("Left {} alone at the owner's request", inc.title);
                self.row(&mut inc, ActionKind::Finished, summary, false, now).await;
                st.dismissed.extend(inc.ever.iter().cloned());
                self.finish(&mut inc);
                self.settle(&mut st, inc, refused);
            }
        }
        Ok(())
    }

    fn put_back(&self, st: &mut State, inc: Incident, refused: bool) {
        if refused {
            st.refused.insert(inc.primary.clone(), inc);
        } else {
            st.incidents.push(inc);
        }
    }

    /// A closed incident: a refusal is simply forgotten; an incident stays in the list until
    /// the next pass moves its fault generation on.
    fn settle(&self, st: &mut State, inc: Incident, refused: bool) {
        if refused {
            st.bump(&inc);
        } else {
            st.incidents.push(inc);
        }
    }
}
