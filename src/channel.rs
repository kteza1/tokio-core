//! In-memory evented channels.
//!
//! This module contains a `Sender` and `Receiver` pair types which can be used
//! to send messages between different future tasks.

use std::io;
use std::sync::mpsc::TryRecvError;

use futures::{Poll, Async};
use futures::stream::Stream;
use mio::channel::{self, TrySendError};

use reactor::{Handle, PollEvented};

/// The transmission half of a channel used for sending messages to a receiver.
///
/// A `Sender` can be `clone`d to have multiple threads or instances sending
/// messages to one receiver.
///
/// This type is created by the [`channel`] function.
///
/// [`channel`]: fn.channel.html
pub struct Sender<T> {
    tx: channel::Sender<T>,
}

/// The transmission half of a synchronous channel used for sending messages to a receiver.
///
/// A `SyncSender` can be `clone`d to have multiple threads or instances sending
/// messages to one receiver.
///
/// This type is created by the [`sync_channel`] function.
///
/// [`sync_channel`]: fn.sync_channel.html
pub struct SyncSender<T> {
    tx: channel::SyncSender<T>,
}

/// The receiving half of a channel used for processing messages sent by a
/// `Sender`.
///
/// A `Receiver` cannot be cloned, so only one thread can receive messages at a
/// time.
///
/// This type is created by the [`channel`] function and implements the
/// `Stream` trait to represent received messages.
///
/// [`channel`]: fn.channel.html
pub struct Receiver<T> {
    rx: PollEvented<channel::Receiver<T>>,
}

/// Creates a new in-memory channel used for sending data across `Send +
/// 'static` boundaries, frequently threads.
///
/// This type can be used to conveniently send messages between futures.
/// Unlike the futures crate `channel` method and types, the returned tx/rx
/// pair is a multi-producer single-consumer (mpsc) channel *with no
/// backpressure*. Currently it's left up to the application to implement a
/// mechanism, if necessary, to avoid messages piling up.
///
/// The returned `Sender` can be used to send messages that are processed by
/// the returned `Receiver`. The `Sender` can be cloned to send messages
/// from multiple sources simultaneously.
pub fn channel<T>(handle: &Handle) -> io::Result<(Sender<T>, Receiver<T>)>
    where T: Send + 'static,
{
    let (tx, rx) = channel::channel();
    let rx = try!(PollEvented::new(rx, handle));
    Ok((Sender { tx: tx }, Receiver { rx: rx }))
}

/// Creates a new in-memory bounded channel used for sending data across `Send +
/// 'static` boundaries, frequently threads.
///
/// This type can be used to conveniently send messages between futures.
/// Unlike the futures crate `channel` method and types, the returned tx/rx
/// pair is a multi-producer single-consumer (mpsc) channel *with no
/// backpressure*. Currently it's left up to the application to implement a
/// mechanism, if necessary, to avoid messages piling up.
///
/// The returned `SyncSender` can be used to send messages that are processed by
/// the returned `Receiver`. The `SyncSender` can be cloned to send messages
/// from multiple sources simultaneously.
pub fn sync_channel<T>(bound: usize, handle: &Handle) -> io::Result<(SyncSender<T>, Receiver<T>)>
    where T: Send + 'static,
{
    let (tx, rx) = channel::sync_channel(bound);
    let rx = try!(PollEvented::new(rx, handle));
    Ok((SyncSender { tx: tx }, Receiver { rx: rx }))
}

impl<T> Sender<T> {
    /// Sends a message to the corresponding receiver of this sender.
    ///
    /// The message provided will be enqueued on the channel immediately, and
    /// this function will return immediately. Keep in mind that the
    /// underlying channel has infinite capacity, and this may not always be
    /// desired.
    ///
    /// If an I/O error happens while sending the message, or if the receiver
    /// has gone away, then an error will be returned. Note that I/O errors here
    /// are generally quite abnormal.
    pub fn send(&self, t: T) -> io::Result<()> {
        self.tx.send(t).map_err(|e| {
            match e {
                channel::SendError::Io(e) => e,
                channel::SendError::Disconnected(_) => {
                    io::Error::new(io::ErrorKind::Other,
                                   "channel has been disconnected")
                }
            }
        })
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Sender<T> {
        Sender { tx: self.tx.clone() }
    }
}

impl<T> SyncSender<T> {
    /// Sends a message to the corresponding receiver of this sender.
    ///
    /// This function will block until space in the internal buffer becomes available.
    /// Otherwise, the message provided will be enqueued on the channel immediately, and
    /// this function will return immediately.
    ///
    /// If an I/O error happens while sending the message, or if the receiver
    /// has gone away, then an error will be returned. Note that I/O errors here
    /// are generally quite abnormal.
    pub fn send(&self, t: T) -> io::Result<()> {
        self.tx.send(t).map_err(|e| {
            match e {
                channel::SendError::Io(e) => e,
                channel::SendError::Disconnected(_) => {
                    io::Error::new(io::ErrorKind::Other,
                                   "channel has been disconnected")
                }
            }
        })
    }

    /// Sends a message to the corresponding receiver of this sender.
    ///
    /// The message provided will be enqueued on the channel immediately, and
    /// this function will return immediately.
    ///
    /// If an I/O error happens while sending the message, or if the receiver
    /// has gone away, or the buffer is full, then an error will be returned.
    /// Note that I/O errors here are generally quite abnormal.
    pub fn try_send(&self, t: T) -> Result<(), TrySendError<T>> {
        if let Err(e) = self.tx.try_send(t) {
            return Err(e);
        }
        Ok(())
    }
}

impl<T> Clone for SyncSender<T> {
    fn clone(&self) -> SyncSender<T> {
        SyncSender { tx: self.tx.clone() }
    }
}

impl<T> Stream for Receiver<T> {
    type Item = T;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<T>, io::Error> {
        if let Async::NotReady = self.rx.poll_read() {
            return Ok(Async::NotReady)
        }
        match self.rx.get_ref().try_recv() {
            Ok(t) => Ok(Async::Ready(Some(t))),
            Err(TryRecvError::Empty) => {
                self.rx.need_read();
                Ok(Async::NotReady)
            }
            Err(TryRecvError::Disconnected) => Ok(Async::Ready(None)),
        }
    }
}
