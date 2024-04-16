use super::varint::VarInt;
use getset::{CopyGetters, Getters, MutGetters, Setters};
use std::{net::SocketAddr, time::Duration};

/// Ref. https://www.iana.org/assignments/quic/quic.xhtml

// QUIC的config配置
#[derive(Getters, Setters, MutGetters, CopyGetters, Debug)]
pub struct TransportParameters {
    // TODO: 得是ConnectionId类型
    #[getset(get = "pub", set = "pub")]
    original_destination_connection_id: Option<Vec<u8>>,
    #[getset(get_copy = "pub", set = "pub")]
    max_idle_timeout: Duration,

    // TODO: 得是Token类型
    #[getset(get = "pub", set = "pub")]
    statelss_reset_token: Option<Vec<u8>>,
    #[getset(get_copy = "pub", set = "pub")]
    max_udp_payload_size: VarInt,
    #[getset(get_copy = "pub", set = "pub")]
    initial_max_data: VarInt,
    #[getset(get_copy = "pub", set = "pub")]
    initial_max_stream_data_bidi_local: VarInt,
    #[getset(get_copy = "pub", set = "pub")]
    initial_max_stream_data_bidi_remote: VarInt,
    #[getset(get_copy = "pub", set = "pub")]
    initial_max_stream_data_uni: VarInt,
    #[getset(get_copy = "pub", set = "pub")]
    initial_max_streams_bidi: VarInt,
    #[getset(get_copy = "pub", set = "pub")]
    initial_max_streams_uni: VarInt,
    #[getset(get_copy = "pub", set = "pub")]
    ack_delay_exponent: u8,
    #[getset(get_copy = "pub", set = "pub")]
    max_ack_delay: u64,
    #[getset(get_copy = "pub", set = "pub")]
    disable_active_migration: bool,
    #[getset(get_copy = "pub", set = "pub")]
    preferred_address: Option<SocketAddr>,
    #[getset(get_copy = "pub", set = "pub")]
    active_connection_id_limit: u64,

    // TODO: 得是ConnectionId类型
    #[getset(get = "pub", set = "pub")]
    initial_source_connection_id: Option<Vec<u8>>,
    #[getset(get = "pub", set = "pub")]
    retry_source_connection_id: Option<Vec<u8>>,
    #[getset(get = "pub", set = "pub")]
    version_information: Option<Vec<u8>>,
    #[getset(get_copy = "pub", set = "pub")]
    max_datagram_frame_size: VarInt,
    #[getset(get_copy = "pub", set = "pub")]
    grease_quic_bit: bool,
}
