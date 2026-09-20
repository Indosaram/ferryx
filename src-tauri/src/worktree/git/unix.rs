use std::io;
#[path = "drain.rs"]
mod drain;

pub(super) struct Owner(libc::pid_t);
pub(super) struct Exit(libc::pid_t);

impl Owner {
    pub fn prepare(command: &mut tokio::process::Command) -> io::Result<Self> {
        // setpgid runs in the child before exec; never infer a group from a PID
        // for a command that was spawned in the daemon's own process group.
        command.process_group(0);
        Ok(Self(0))
    }

    pub fn attach(mut self, child: &tokio::process::Child) -> io::Result<Self> {
        let pid = child.id().expect("spawned Git PID") as libc::pid_t;
        let group = unsafe { libc::getpgid(pid) };
        if group != pid {
            return Err(if group == -1 {
                io::Error::last_os_error()
            } else {
                io::Error::other("Git did not enter its owned process group")
            });
        }
        self.0 = pid;
        Ok(self)
    }

    pub fn exit_observer(&self) -> Exit {
        Exit(self.0)
    }

    pub fn terminate(&self) -> io::Result<()> {
        self.signal()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let members = drain::members(self.0)?;
            if members.is_empty() {
                return Ok(());
            }
            let watches = members
                .into_iter()
                .map(|pid| {
                    let watch = drain::subscribe(pid)?;
                    // A disappearing member's PID could be reused between inventory
                    // and subscription. Never wait on that unrelated replacement.
                    if unsafe { libc::getpgid(pid) } != self.0 {
                        return Ok(None);
                    }
                    Ok(watch)
                })
                .collect::<io::Result<Vec<_>>>()?;
            self.signal()?;
            for watch in watches.into_iter().flatten() {
                drain::wait(watch, deadline)?;
            }
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Git group drain deadline",
                ));
            }
        }
    }

    fn signal(&self) -> io::Result<()> {
        if unsafe { libc::kill(-self.0, libc::SIGKILL) } == -1 {
            let error = io::Error::last_os_error();
            #[cfg(target_os = "macos")]
            if error.raw_os_error() == Some(libc::EPERM) && self.only_zombies()? {
                return Ok(());
            }
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn only_zombies(&self) -> io::Result<bool> {
        // Darwin killpg reports EPERM for a group containing only zombies.
        // Do not blanket-ignore EPERM: inspect this exact anchored group.
        let mut pids = vec![0i32; 65536];
        let bytes = unsafe {
            libc::proc_listpids(
                2,
                self.0 as u32,
                pids.as_mut_ptr().cast(),
                (pids.len() * 4) as i32,
            )
        };
        if bytes <= 0 {
            return Err(io::Error::last_os_error());
        }
        if bytes as usize >= pids.len() * 4 {
            return Err(io::Error::other("Git group inventory exceeded limit"));
        }
        for pid in pids
            .into_iter()
            .take(bytes as usize / 4)
            .filter(|pid| *pid > 0)
        {
            let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of_val(&info) as i32;
            if unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDTBSDINFO,
                    0,
                    (&mut info as *mut libc::proc_bsdinfo).cast(),
                    size,
                )
            } != size
            {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ESRCH) {
                    continue;
                }
                return Err(error);
            }
            #[cfg(test)]
            eprintln!(
                "A08 Darwin EPERM owned_pgid={} pid={pid} actual_pgid={} uid={} status={}",
                self.0, info.pbi_pgid, info.pbi_uid, info.pbi_status
            );
            if info.pbi_pgid != self.0 as u32 || info.pbi_status != libc::SZOMB {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl Exit {
    pub fn wait(self) -> io::Result<()> {
        // Event observation retains the unreaped leader as the ownership anchor.
        // Unlike blocking waitid, the worker itself has a finite lifetime even
        // if signaling fails and its async caller times out.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
        if let Some(watch) = drain::subscribe(self.0)? {
            drain::wait(watch, deadline)?;
        }
        Ok(())
    }
}
