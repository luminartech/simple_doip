//! The embedded-server TX hot path: encode a DoIP header + diagnostic payload into ONE
//! stack buffer, no staging buffer, using encoded_size to pre-size the header
//! (automotive-wire-codec spec §7.2 pattern).
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
    let mut writer: &mut [u8] = &mut tx_buf;
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

    // 4. Too-small buffer errors recoverably (no panic): embedded-io's `&mut [u8]`
    //    writer surfaces exhaustion as `Io(WriteZero)` — recoverable per the tier
    //    classifier (S3 finding, pinned here).
    let mut small = [0u8; 4];
    let mut w: &mut [u8] = &mut small;
    let err = header.encode(&mut w).unwrap_err();
    assert!(matches!(
        err,
        MessageError::Io(embedded_io::ErrorKind::WriteZero)
    ));
    assert!(!err.is_framing_fatal());
}
