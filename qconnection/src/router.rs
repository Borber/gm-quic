use std::sync::{Arc, LazyLock};

use dashmap::DashMap;
use deref_derive::Deref;
use qbase::{
    cid::{ConnectionId, UniqueCid},
    error::Error,
    frame::{NewConnectionIdFrame, ReceiveFrame, RetireConnectionIdFrame, SendFrame},
    packet::{header::GetDcid, long, DataHeader, DataPacket},
};
use qudp::ArcUsc;

use crate::{connection::PacketEntry, path::pathway::Pathway};

/// Global Router for managing connections.
pub static ROUTER: LazyLock<ArcRouter> = LazyLock::new(|| ArcRouter(Arc::new(DashMap::new())));

#[derive(Clone, Deref, Debug)]
pub struct ArcRouter(Arc<DashMap<ConnectionId, [PacketEntry; 4]>>);

impl UniqueCid for ArcRouter {
    fn is_unique_cid(&self, cid: &ConnectionId) -> bool {
        self.0.get(cid).is_none()
    }
}

impl ArcRouter {
    pub fn recv_packet_via_pathway(
        &self,
        packet: DataPacket,
        pathway: Pathway,
        usc: &ArcUsc,
    ) -> Option<DataPacket> {
        let dcid = packet.header.get_dcid();
        if let Some(entries) = self.0.get(dcid) {
            let index = match packet.header {
                DataHeader::Long(long::DataHeader::Initial(_)) => 0,
                DataHeader::Long(long::DataHeader::ZeroRtt(_)) => 1,
                DataHeader::Long(long::DataHeader::Handshake(_)) => 2,
                DataHeader::Short(_) => 3,
            };
            _ = entries[index].unbounded_send((packet, pathway, usc.clone()));
            None
        } else {
            Some(packet)
        }
    }

    pub fn registry<ISSUED>(
        &self,
        scid: ConnectionId,
        issued_cids: ISSUED,
        packet_entries: [PacketEntry; 4],
    ) -> RouterRegistry<ISSUED>
    where
        ISSUED: SendFrame<NewConnectionIdFrame>,
    {
        self.0.insert(scid, packet_entries.clone());
        RouterRegistry {
            router: self.clone(),
            issued_cids,
            packet_entries,
        }
    }

    pub fn revoke<T>(&self, local_cids: T) -> RevokeRouter<T> {
        RevokeRouter {
            router: self.clone(),
            local_cids,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RouterRegistry<ISSUED> {
    router: ArcRouter,
    issued_cids: ISSUED,
    packet_entries: [PacketEntry; 4],
}

impl<T> SendFrame<NewConnectionIdFrame> for RouterRegistry<T>
where
    T: SendFrame<NewConnectionIdFrame>,
{
    fn send_frame<I: IntoIterator<Item = NewConnectionIdFrame>>(&self, iter: I) {
        self.issued_cids
            .send_frame(iter.into_iter().inspect(|frame| {
                self.router.insert(frame.id, self.packet_entries.clone());
            }))
    }
}

impl<T> UniqueCid for RouterRegistry<T> {
    fn is_unique_cid(&self, cid: &ConnectionId) -> bool {
        self.router.is_unique_cid(cid)
    }
}

#[derive(Clone)]
pub struct RevokeRouter<T> {
    router: ArcRouter,
    local_cids: T,
}

impl<T> ReceiveFrame<RetireConnectionIdFrame> for RevokeRouter<T>
where
    T: ReceiveFrame<RetireConnectionIdFrame, Output = Option<ConnectionId>>,
{
    type Output = ();

    fn recv_frame(&self, frame: &RetireConnectionIdFrame) -> Result<Self::Output, Error> {
        if let Some(cid) = self.local_cids.recv_frame(frame)? {
            self.router.remove(&cid);
        }
        Ok(())
    }
}
