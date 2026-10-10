//! Requests never run on the UI thread. A worker takes one piece of work at a
//! time, in order, and hands the result back into the event loop.

use super::{App, with_app};
use crate::api::Client;
use std::sync::mpsc;

type Done = Box<dyn FnOnce(&mut App) + Send>;
type Work = Box<dyn FnOnce(&Client) -> Done + Send>;

pub struct Worker {
    queue: mpsc::Sender<Work>,
}

impl Worker {
    /// `session` marks whose answers these are: after a sign-out the late
    /// ones of the old session are dropped.
    pub fn new(client: Client, session: u64) -> Self {
        let (queue, work) = mpsc::channel::<Work>();
        std::thread::spawn(move || {
            for work in work {
                let done = work(&client);
                let _ = slint::invoke_from_event_loop(move || {
                    with_app(move |app| {
                        if app.session == session {
                            done(app);
                        }
                    });
                });
            }
        });
        Self { queue }
    }

    pub fn ask<T: Send + 'static>(&self, work: impl FnOnce(&Client) -> T + Send + 'static, done: impl FnOnce(&mut App, T) + Send + 'static) {
        let _ = self.queue.send(Box::new(move |client| {
            let result = work(client);
            Box::new(move |app| done(app, result))
        }));
    }
}
