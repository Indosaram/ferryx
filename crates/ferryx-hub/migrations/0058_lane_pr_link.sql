-- T7: the lane's pull request, as the last read-only `gh pr view` saw it.
--
-- Number 0057 is assigned to T7 by `.omo/plans/t3code-control-plane-port.md` §2; 0054/0055/0056
-- belong to T1/T2/T5 and land in their own changes, so the gap is deliberate, not a missing file.
--
-- This is a projection, never the authority: GitHub owns the PR state, and Ferryx only records
-- what one `gh pr view` answered so the tracker can show a lane's PR (and T6 can settle on a
-- merged PR) without a network read on every render. Nothing here is written by a mutating `gh`
-- command; a state change is only ever observed again, never pushed to GitHub.
--
-- One row per lane: the link is the lane's current PR, replaced when a sync sees another one.
-- `state` mirrors GitHub's own PR lifecycle; a PR merged outside Ferryx flips to 'merged' on the
-- next read and the row is left otherwise untouched.
CREATE TABLE IF NOT EXISTS lane_pull_requests (
    lane_key TEXT PRIMARY KEY REFERENCES lanes(key),
    repo TEXT NOT NULL,
    pr_number INTEGER NOT NULL CHECK(pr_number > 0),
    state TEXT NOT NULL CHECK(state IN ('open', 'closed', 'merged')),
    url TEXT NOT NULL,
    title TEXT NOT NULL,
    head_branch TEXT NOT NULL,
    observed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_lane_pull_requests_state ON lane_pull_requests(state);
