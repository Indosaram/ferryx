# Mac recovered frontend findings
App.test.tsx: candidate3failed151passed; startup epoch mismatch and missing-HMR replacement failures absent from base failure names. Welcome-dialog failure matches base. Mixed candidate/baseline file; frontend reopened.
App.remote.test.tsx: same named remote reattach test fails candidate on missing getCurrentVersion mock export, base on missing subscribeUpdateStatus. Changed baseline-masked assertion; not exact pre-existing proof. Frontend reopened.
App.pairedDaemon.test.tsx: candidate1failed7passed native1 against base8passed native0. Candidate-caused file; full assertion recovery archive pending. Frontend reopened.
NativeTerminalPane.presentation.test.tsx: candidate3failed11passed native1 against base14passed native0. Candidate-caused file; full assertion recovery archive pending. Frontend reopened.
NativeTerminalPane.test.tsx:186 matching named explicit failure blocks candidate/base,197selected each; pre-existing recovered assertions.
All these file runs are supplemental; Mac full UI remains TIMED_OUT1200s/SIGKILL with final selected count unknown.
