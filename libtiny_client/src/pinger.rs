//! Implements two-state "pinger" task that drives sending pings to the server to check liveness of
//! the connection.

use futures::FutureExt;
use futures::{pin_mut, select, stream::StreamExt};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::timer::delay_for;

pub(crate) struct Pinger {
    snd_rst: mpsc::Sender<()>,
}

#[derive(Debug)]
pub(crate) enum Event {
    SendPing,
    Disconnect,
}

enum PingerState {
    /// Signal a "ping" on timeout. State moves to `ExpectPong`.
    SendPing,
    /// Signal a "disconnect" on timeout.
    ExpectPong,
}

async fn pinger_task(rcv_rst: mpsc::Receiver<()>, mut snd_ev: mpsc::Sender<Event>) {
    let mut rcv_rst_fused = rcv_rst.fuse();
    let mut state = PingerState::SendPing;
    loop {
        // NOTE: The code about does not work:
        // let mut delay = delay_for(Duration::from_secs(30));
        // Instead I need this weird code below. Not sure if this is a bug or not.
        let delay = async {
            delay_for(Duration::from_secs(30)).await;
        }
            .fuse();
        pin_mut!(delay);

        eprintln!("pinger: select");
        select! {
            () = delay => {
                eprintln!("pinger: delay yielded");
                match state {
                    PingerState::SendPing => {
                        state = PingerState::ExpectPong;
                        eprintln!("pinger: SendPing");
                        snd_ev.try_send(Event::SendPing).unwrap();
                    }
                    PingerState::ExpectPong => {
                        eprintln!("pinger: Disconnect");
                        snd_ev.try_send(Event::Disconnect).unwrap();
                        return;
                    }
                }
            }
            cmd = rcv_rst_fused.next() => {
                eprintln!("pinger: rcv_rst yielded");
                match cmd {
                    None => {
                        eprintln!("pinger: Return");
                        return;
                    }
                    Some(()) => {
                        eprintln!("pinger: Reset");
                        state = PingerState::SendPing;
                    }
                }
            }
        }
    }
}

impl Pinger {
    pub(crate) fn new() -> (Pinger, mpsc::Receiver<Event>) {
        let (snd_ev, rcv_ev) = mpsc::channel(1);
        // No need for sending another "reset" when there's already one waiting to be processed
        let (snd_rst, rcv_rst) = mpsc::channel(1);
        tokio::runtime::current_thread::spawn(pinger_task(rcv_rst, snd_ev));
        (Pinger { snd_rst }, rcv_ev)
    }

    pub(crate) fn reset(&mut self) {
        // Ignore errors: no need to send another "reset" when there's already one waiting to be
        // processed
        let _ = self.snd_rst.try_send(());
    }
}