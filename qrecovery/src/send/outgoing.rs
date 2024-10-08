use std::{
    future::Future,
    io::Error,
    ops::{DerefMut, Range},
    pin::Pin,
    task::{Context, Poll},
};

use bytes::BufMut;
use futures::ready;
use qbase::{
    error::Error as QuicError,
    frame::{io::WriteDataFrame, ShouldCarryLength, StreamFrame},
    streamid::StreamId,
    util::DescribeData,
    varint::VARINT_MAX,
};

use super::sender::{ArcSender, DataSentSender, Sender, SendingSender};

#[derive(Debug, Clone)]
pub struct Outgoing(pub(crate) ArcSender);

impl Outgoing {
    pub fn update_window(&self, max_data_size: u64) {
        assert!(max_data_size <= VARINT_MAX);
        let mut sender = self.0.sender();
        match sender.deref_mut() {
            Ok(Sender::Ready(s)) => s.update_window(max_data_size),
            Ok(Sender::Sending(s)) => s.update_window(max_data_size),
            _ => {}
        }
    }

    pub fn try_read(
        &self,
        sid: StreamId,
        mut buf: &mut [u8],
        tokens: usize,
        flow_limit: usize,
    ) -> Option<(StreamFrame, usize, bool, usize)> {
        let capacity = buf.len();
        let write = |(offset, is_fresh, data, is_eos): (u64, bool, (&[u8], &[u8]), bool)| {
            let mut frame = StreamFrame::new(sid, offset, data.len());

            frame.set_eos_flag(is_eos);
            match frame.should_carry_length(capacity) {
                ShouldCarryLength::NoProblem => {
                    buf.put_data_frame(&frame, &data);
                }
                ShouldCarryLength::PaddingFirst(n) => {
                    (&mut buf[n..]).put_data_frame(&frame, &data);
                }
                ShouldCarryLength::ShouldAfter(_not_carry_len, _carry_len) => {
                    frame.carry_length();
                    buf.put_data_frame(&frame, &data);
                }
            }
            (frame, data.len(), is_fresh, capacity - buf.remaining_mut())
        };

        let predicate = |offset| {
            StreamFrame::estimate_max_capacity(capacity, sid, offset).map(|c| tokens.min(c))
        };
        let mut sender = self.0.sender();
        let inner = sender.deref_mut();

        match inner {
            Ok(sending_state) => match sending_state {
                Sender::Ready(s) => {
                    let result;
                    if s.is_shutdown() {
                        let mut s: DataSentSender = s.into();
                        result = s.pick_up(predicate, flow_limit).map(write);
                        *sending_state = Sender::DataSent(s);
                    } else {
                        let mut s: SendingSender = s.into();
                        result = s.pick_up(predicate, flow_limit).map(write);
                        *sending_state = Sender::Sending(s);
                    }
                    result
                }
                Sender::Sending(s) => s.pick_up(predicate, flow_limit).map(write),
                Sender::DataSent(s) => s.pick_up(predicate, flow_limit).map(write),
                _ => None,
            },
            Err(_) => None,
        }
    }

    /// return true if all data has been rcvd
    pub fn on_data_acked(&self, range: &Range<u64>, is_fin: bool) -> bool {
        let mut sender = self.0.sender();
        let inner = sender.deref_mut();
        if let Ok(sending_state) = inner {
            match sending_state {
                Sender::Ready(_) => {
                    unreachable!("never send data before recv data");
                }
                Sender::Sending(s) => {
                    s.on_data_acked(range);
                }
                Sender::DataSent(s) => {
                    s.on_data_acked(range, is_fin);
                    if s.is_all_rcvd() {
                        *sending_state = Sender::DataRcvd;
                        return true;
                    }
                }
                // ignore recv
                _ => {}
            }
        };
        false
    }

    pub fn may_loss_data(&self, range: &Range<u64>) {
        let mut sender = self.0.sender();
        let inner = sender.deref_mut();
        if let Ok(sending_state) = inner {
            match sending_state {
                Sender::Ready(_) => {
                    unreachable!("never send data before recv data");
                }
                Sender::Sending(s) => {
                    s.may_loss_data(range);
                }
                Sender::DataSent(s) => {
                    s.may_loss_data(range);
                }
                // ignore loss
                _ => (),
            }
        };
    }

    /// 被动stop，返回true说明成功stop了；返回false则表明流没有必要stop，要么已经完成，要么已经reset
    pub fn stop(&self) -> bool {
        let mut sender = self.0.sender();
        let inner = sender.deref_mut();
        match inner {
            Ok(sending_state) => match sending_state {
                Sender::Ready(_) => {
                    unreachable!("never send data before recv data");
                }
                Sender::Sending(s) => {
                    *sending_state = Sender::ResetSent(s.stop());
                    true
                }
                Sender::DataSent(s) => {
                    *sending_state = Sender::ResetSent(s.stop());
                    true
                }
                _ => false,
            },
            Err(_) => false,
        }
    }

    pub fn on_reset_acked(&self) {
        let mut sender = self.0.sender();
        let inner = sender.deref_mut();
        if let Ok(sending_state) = inner {
            match sending_state {
                Sender::ResetSent(_) | Sender::ResetRcvd => {
                    *sending_state = Sender::ResetRcvd;
                }
                _ => {
                    unreachable!(
                    "If no RESET_STREAM has been sent, how can there be a received acknowledgment?"
                );
                }
            }
        }
    }

    /// When a connection-level error occurs, all data streams must be notified.
    /// Their reading and writing should be terminated, accompanied the error of the connection.
    pub fn on_conn_error(&self, err: &QuicError) {
        let mut sender = self.0.sender();
        let inner = sender.deref_mut();
        match inner {
            Ok(sending_state) => match sending_state {
                Sender::Ready(s) => s.wake_all(),
                Sender::Sending(s) => s.wake_all(),
                Sender::DataSent(s) => s.wake_all(),
                _ => return,
            },
            Err(_) => return,
        };
        *inner = Err(Error::new(std::io::ErrorKind::BrokenPipe, err.to_string()));
    }

    pub fn is_cancelled_by_app(&self) -> IsCancelled {
        IsCancelled(&self.0)
    }
}

pub struct IsCancelled<'s>(&'s ArcSender);

impl Future for IsCancelled<'_> {
    // (u64, u64) -> (final_size, err_code)
    type Output = Option<(u64, u64)>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut sender = self.0.sender();
        let inner = sender.deref_mut();
        match inner {
            Ok(sending_state) => match sending_state {
                Sender::Ready(s) => {
                    let (final_size, err_code) = ready!(s.poll_cancel(cx));
                    *sending_state = Sender::ResetSent(final_size);
                    Poll::Ready(Some((final_size, err_code)))
                }
                Sender::Sending(s) => {
                    let (final_size, err_code) = ready!(s.poll_cancel(cx));
                    *sending_state = Sender::ResetSent(final_size);
                    Poll::Ready(Some((final_size, err_code)))
                }
                Sender::DataSent(s) => {
                    let (final_size, err_code) = ready!(s.poll_cancel(cx));
                    *sending_state = Sender::ResetSent(final_size);
                    Poll::Ready(Some((final_size, err_code)))
                }
                _ => Poll::Ready(None),
            },
            // 既然发生连接错误了，那也没必要监听应用层的取消了
            Err(_) => Poll::Ready(None),
        }
    }
}
