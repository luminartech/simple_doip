//! The embedded-server TX hot path: encode a `DoIP` header + diagnostic payload into ONE
//! stack buffer, no staging buffer, using `encoded_size` to pre-size the header.
use automotive_wire_codec::{InsufficientBuffer, SliceSink, WriteError};
use simple_doip::messages::{
    DiagnosticMessage, Encode, Header, MessageError, Payload, PayloadType, ProtocolVersion,
};
use simple_doip::{LogicalAddress, try_frame};

#[test]
fn nested_encode_no_staging_buffer() {
    let uds_response = [0x62u8, 0xF1, 0x90, 0xAA, 0xBB];
    let dm = DiagnosticMessage {
        source_address: LogicalAddress(0x1000),
        target_address: LogicalAddress(0x0E00),
        user_data: &uds_response[..],
    };

    // 1. Size the inner payload first.
    let payload_len = dm.encoded_size().unwrap();
    let header = Header::new(
        ProtocolVersion::V2012,
        PayloadType::DiagnosticMessage,
        u32::try_from(payload_len).unwrap(),
    );

    // 2. One stack buffer, two sequential encodes.
    let mut tx_buf = [0u8; 64];
    let mut writer = SliceSink::new(&mut tx_buf);
    let mut total = header.encode(&mut writer).unwrap();
    total += dm.encode(&mut writer).unwrap();
    assert_eq!(total, Header::SIZE + payload_len);

    // 3. Frame it back out and decode the payload — full loop.
    let (frame, consumed) = try_frame(&tx_buf[..total]).unwrap().unwrap();
    assert_eq!(consumed, total);
    let decoded = Payload::decode(frame.payload, frame.header.payload_type).unwrap();
    match decoded {
        Payload::DiagnosticMessage(d) => assert_eq!(d.user_data, &uds_response[..]),
        other => panic!("wrong payload: {other:?}"),
    }

    // 4. Too-small buffer errors recoverably (no panic): a `SliceSink` surfaces
    //    exhaustion as `Io(WriteError::Insufficient(..))` with counts attached
    //    — recoverable per the tier classifier. Pinned here so a future change
    //    cannot turn buffer exhaustion on this path into a panic or a
    //    framing-fatal error. `needed_at_least` (8) is a lower bound: 4 bytes
    //    already written plus the 4-byte `write_u32_be` that failed, not the
    //    header's full 8-byte size (which happens to coincide here).
    let mut small = [0u8; 4];
    let mut w = SliceSink::new(&mut small);
    let err = header.encode(&mut w).unwrap_err();
    assert!(matches!(
        err,
        MessageError::Io(WriteError::Insufficient(InsufficientBuffer {
            needed_at_least: 8,
            available: 4
        }))
    ));
    assert!(!err.is_framing_fatal());
}
