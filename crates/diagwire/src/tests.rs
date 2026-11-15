//! DR-0001 D10 scenarios on a fake stream and tokio's paused clock.
//!
//! Every byte here is hand-written. These tests characterize what diagwire
//! believes the protocol to be (D4); none of them is conformance evidence.
//! Frames are built by hand, not with the `hsfz` codec under test.

use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex};
use tokio::time::{Duration, Instant, sleep, timeout};

use super::*;

const TESTER: u8 = 0xF4;
const DME: u8 = 0x12;
const EGS: u8 = 0x18;
const MASK: u8 = 0xFF;

const DIAG: u16 = 0x0001;
const ACK: u16 = 0x0002;
const ALIVE: u16 = 0x0012;

/// `19 02 FF` from the tester to the DME, as diagwire is expected to send it.
const REQUEST: [u8; 11] = [0, 0, 0, 5, 0, 1, TESTER, DME, 0x19, 0x02, MASK];

/// A positive response with two fault records.
const TWO_FAULTS: [u8; 11] = [0x59, 0x02, 0xFF, 0x12, 0x34, 0x56, 0x2F, 0xAB, 0xCD, 0xEF, 0x08];
/// A positive response with one, different, fault record.
const ONE_FAULT: [u8; 7] = [0x59, 0x02, 0xFF, 0x0A, 0x1B, 0x2C, 0x01];
const PENDING: [u8; 3] = [0x7F, 0x19, 0x78];

fn frame(control: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = (payload.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&control.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

fn diag(source: u8, target: u8, data: &[u8]) -> Vec<u8> {
    let mut payload = vec![source, target];
    payload.extend_from_slice(data);
    frame(DIAG, &payload)
}

/// A reply from the DME to the tester.
fn reply(data: &[u8]) -> Vec<u8> {
    diag(DME, TESTER, data)
}

fn ack() -> Vec<u8> {
    frame(ACK, &REQUEST[6..])
}

fn codes(report: &uds::DtcReport) -> Vec<(u32, u8)> {
    report.dtcs.iter().map(|d| (d.code, d.status.0)).collect()
}

/// The gateway's end of the fake link.
struct Gateway(DuplexStream);

impl Gateway {
    /// Reads exactly `bytes.len()` bytes and asserts what they are.
    async fn expect(&mut self, bytes: &[u8]) {
        let mut got = vec![0u8; bytes.len()];
        self.0.read_exact(&mut got).await.expect("the tester closed the link early");
        assert_eq!(got, bytes, "bytes the tester sent");
    }

    async fn send(&mut self, bytes: &[u8]) {
        self.0.write_all(bytes).await.expect("the tester closed the link");
    }

    /// Expects the request and acks it.
    async fn accept_request(&mut self) {
        self.expect(&REQUEST).await;
        self.send(&ack()).await;
    }

    /// Asserts the tester closed its end without sending anything more.
    async fn expect_closed(&mut self) {
        let mut rest = Vec::new();
        self.0.read_to_end(&mut rest).await.unwrap();
        assert_eq!(rest, [] as [u8; 0], "bytes sent before the tester closed the link");
    }
}

/// Runs `read` and reports how long it took on its own, whatever else is
/// joined with it.
async fn timed<T>(read: impl Future<Output = T>) -> (T, Duration) {
    let start = Instant::now();
    let out = read.await;
    (out, start.elapsed())
}

fn link() -> (Session<DuplexStream>, Gateway) {
    let (ours, theirs) = duplex(4096);
    (Session::from_stream(ours), Gateway(theirs))
}

/// Asserts the connection is closed and stays closed: the link is shut with
/// nothing further on it, and a later read fails without touching it.
async fn assert_disposed(session: &mut Session<DuplexStream>, gw: &mut Gateway) {
    assert!(session.is_disposed());
    gw.expect_closed().await;
    assert!(matches!(
        session.read_dtcs(DME, MASK).await,
        Err(Error::Transport(hsfz::Error::Disposed))
    ));
}

#[tokio::test(start_paused = true)]
async fn complete_read_keeps_the_connection() {
    let (mut session, mut gw) = link();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.accept_request().await;
        gw.send(&reply(&TWO_FAULTS)).await;
    });
    let Outcome::Complete(report) = outcome.unwrap() else {
        panic!("expected a complete read");
    };
    assert_eq!(report.availability_mask, 0xFF);
    assert_eq!(codes(&report), [(0x123456, 0x2F), (0xABCDEF, 0x08)]);
    assert!(!session.is_disposed());

    // The same connection serves a second transaction.
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.accept_request().await;
        gw.send(&reply(&ONE_FAULT)).await;
    });
    let Outcome::Complete(report) = outcome.unwrap() else {
        panic!("expected a complete read");
    };
    assert_eq!(codes(&report), [(0x0A1B2C, 0x01)]);
}

#[tokio::test(start_paused = true)]
async fn no_faults_is_a_complete_read() {
    let (mut session, mut gw) = link();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.accept_request().await;
        gw.send(&reply(&[0x59, 0x02, 0xFF])).await;
    });
    let Outcome::Complete(report) = outcome.unwrap() else {
        panic!("expected a complete read");
    };
    assert_eq!(codes(&report), []);
}

/// Replay 1: the gateway's ack never arrives.
#[tokio::test(start_paused = true)]
async fn missing_ack_is_no_response_and_closes() {
    let (mut session, mut gw) = link();
    let start = Instant::now();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), gw.expect(&REQUEST));
    assert_eq!(outcome.unwrap(), Outcome::NoResponse);
    assert_eq!(start.elapsed(), Duration::from_secs(2));
    assert_disposed(&mut session, &mut gw).await;
}

/// The deadline covers the write: a gateway that never reads cannot stall
/// the operation.
#[tokio::test(start_paused = true)]
async fn blocked_write_is_bounded_by_the_deadline() {
    // Room for 4 of the request's 11 bytes, and nobody reading.
    let (ours, theirs) = duplex(4);
    let mut session = Session::from_stream(ours);
    let mut gw = Gateway(theirs);
    let start = Instant::now();
    assert_eq!(session.read_dtcs(DME, MASK).await.unwrap(), Outcome::NoResponse);
    assert_eq!(start.elapsed(), Duration::from_secs(2));
    assert!(session.is_disposed());
    // The interrupted write left a partial request, and then the link closed.
    let mut partial = Vec::new();
    gw.0.read_to_end(&mut partial).await.unwrap();
    assert_eq!(partial, REQUEST[..4]);
}

#[tokio::test(start_paused = true)]
async fn silence_after_the_ack_is_no_response() {
    let (mut session, mut gw) = link();
    let start = Instant::now();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), gw.accept_request());
    assert_eq!(outcome.unwrap(), Outcome::NoResponse);
    assert_eq!(start.elapsed(), Duration::from_secs(2));
    assert_disposed(&mut session, &mut gw).await;
}

#[tokio::test(start_paused = true)]
async fn pending_extends_the_wait_for_the_final_reply() {
    let (mut session, mut gw) = link();
    let start = Instant::now();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.accept_request().await;
        sleep(Duration::from_secs(1)).await;
        gw.send(&reply(&PENDING)).await;
        // 4 s after the pending reply: past the first-response wait, inside
        // the pending wait.
        sleep(Duration::from_secs(4)).await;
        gw.send(&reply(&TWO_FAULTS)).await;
    });
    let Outcome::Complete(report) = outcome.unwrap() else {
        panic!("expected a complete read");
    };
    assert_eq!(codes(&report), [(0x123456, 0x2F), (0xABCDEF, 0x08)]);
    assert_eq!(start.elapsed(), Duration::from_secs(5));
    assert!(!session.is_disposed());
}

#[tokio::test(start_paused = true)]
async fn pending_then_silence_is_pending_expired() {
    let (mut session, mut gw) = link();
    let start = Instant::now();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.accept_request().await;
        gw.send(&reply(&PENDING)).await;
    });
    assert_eq!(outcome.unwrap(), Outcome::PendingExpired);
    assert_eq!(start.elapsed(), Duration::from_secs(5));
    assert_disposed(&mut session, &mut gw).await;
}

/// No pending reply extends the operation past its deadline.
#[tokio::test(start_paused = true)]
async fn endless_pending_stops_at_the_operation_deadline() {
    let (mut session, mut gw) = link();
    let ((outcome, took), sent) = tokio::join!(timed(session.read_dtcs(DME, MASK)), async {
        gw.accept_request().await;
        let mut sent = 0;
        // Every 4 s, inside each 5 s pending wait, until the tester hangs up.
        while gw.0.write_all(&reply(&PENDING)).await.is_ok() {
            sent += 1;
            sleep(Duration::from_secs(4)).await;
        }
        sent
    });
    assert_eq!(outcome.unwrap(), Outcome::PendingExpired);
    assert_eq!(took, Duration::from_secs(30));
    assert_eq!(sent, 8, "pending replies at 0, 4, .. 28 s");
    assert_disposed(&mut session, &mut gw).await;
}

/// First-slice scenario 7 (replay 3): a frame is split across reads and the
/// deadline fires in the middle of it.
#[tokio::test(start_paused = true)]
async fn deadline_inside_a_split_frame_closes_the_connection() {
    let (mut session, mut gw) = link();
    let response = reply(&TWO_FAULTS);
    let ((outcome, took), late_write) = tokio::join!(timed(session.read_dtcs(DME, MASK)), async {
        gw.accept_request().await;
        sleep(Duration::from_secs(1)).await;
        gw.send(&response[..9]).await;
        // The rest arrives after the 2 s response deadline.
        sleep(Duration::from_secs(2)).await;
        gw.0.write_all(&response[9..]).await
    });
    assert_eq!(outcome.unwrap(), Outcome::NoResponse);
    assert_eq!(took, Duration::from_secs(2));
    // The late half had nowhere to go, so no later call can see it.
    assert!(late_write.is_err());
    assert_disposed(&mut session, &mut gw).await;
}

#[tokio::test(start_paused = true)]
async fn impossible_frame_length_is_malformed_and_closes() {
    let (mut session, mut gw) = link();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.accept_request().await;
        gw.send(&[0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x01]).await;
    });
    assert_eq!(outcome.unwrap(), Outcome::Malformed);
    assert_disposed(&mut session, &mut gw).await;
}

#[tokio::test(start_paused = true)]
async fn diagnostic_frame_without_addresses_is_malformed() {
    let (mut session, mut gw) = link();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.accept_request().await;
        gw.send(&frame(DIAG, &[DME])).await;
    });
    assert_eq!(outcome.unwrap(), Outcome::Malformed);
    assert_disposed(&mut session, &mut gw).await;
}

/// A response counts only if the addressed ECU sent it to the tester.
#[tokio::test(start_paused = true)]
async fn replies_from_or_to_anyone_else_do_not_count() {
    let (mut session, mut gw) = link();
    let start = Instant::now();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.accept_request().await;
        gw.send(&diag(EGS, TESTER, &TWO_FAULTS)).await;
        gw.send(&diag(DME, 0xF5, &TWO_FAULTS)).await;
    });
    assert_eq!(outcome.unwrap(), Outcome::NoResponse);
    assert_eq!(start.elapsed(), Duration::from_secs(2));
    assert_disposed(&mut session, &mut gw).await;

    // With the real answer after them, the read returns that answer.
    let (mut session, mut gw) = link();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.accept_request().await;
        gw.send(&diag(EGS, TESTER, &TWO_FAULTS)).await;
        gw.send(&diag(DME, 0xF5, &TWO_FAULTS)).await;
        gw.send(&reply(&ONE_FAULT)).await;
    });
    let Outcome::Complete(report) = outcome.unwrap() else {
        panic!("expected a complete read");
    };
    assert_eq!(codes(&report), [(0x0A1B2C, 0x01)]);
}

/// Each of these is from the right ECU and still not a valid answer to
/// `19 02`. The last is replay 8: no partial fault list comes back.
#[tokio::test(start_paused = true)]
async fn invalid_responses_are_malformed_and_close() {
    let cases: [(&str, &[u8]); 7] = [
        ("positive SID of another service", &[0x62, 0x02, 0xFF]),
        ("negative response to another service", &[0x7F, 0x22, 0x31]),
        ("pending for another service", &[0x7F, 0x22, 0x78]),
        ("another subfunction echoed", &[0x59, 0x0A, 0xFF]),
        ("no UDS bytes", &[]),
        ("negative response cut short", &[0x7F, 0x19]),
        ("record count not a multiple of 4", &[0x59, 0x02, 0xFF, 0x12, 0x34, 0x56, 0x2F, 0xAB, 0xCD]),
    ];
    for (what, data) in cases {
        let (mut session, mut gw) = link();
        let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
            gw.accept_request().await;
            gw.send(&reply(data)).await;
        });
        assert_eq!(outcome.unwrap(), Outcome::Malformed, "{what}");
        assert_disposed(&mut session, &mut gw).await;
    }
}

#[tokio::test(start_paused = true)]
async fn negative_response_keeps_the_connection() {
    let (mut session, mut gw) = link();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.accept_request().await;
        gw.send(&reply(&[0x7F, 0x19, 0x31])).await;
    });
    assert_eq!(outcome.unwrap(), Outcome::Negative(0x31));
    assert!(!session.is_disposed());

    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.accept_request().await;
        gw.send(&reply(&ONE_FAULT)).await;
    });
    assert!(matches!(outcome.unwrap(), Outcome::Complete(_)));
}

/// Replay 4: an alive check arrives while a send waits for its ack, and
/// again while the response is awaited. The reply's format is unverified.
#[tokio::test(start_paused = true)]
async fn alive_checks_are_answered_inside_a_transaction() {
    let alive_reply = frame(ALIVE, &[0x00, TESTER]);
    let (mut session, mut gw) = link();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.expect(&REQUEST).await;
        gw.send(&frame(ALIVE, &[])).await;
        gw.expect(&alive_reply).await;
        gw.send(&ack()).await;
        gw.send(&frame(ALIVE, &[])).await;
        gw.expect(&alive_reply).await;
        gw.send(&reply(&ONE_FAULT)).await;
    });
    assert!(matches!(outcome.unwrap(), Outcome::Complete(_)));
    assert!(!session.is_disposed());
}

/// Nothing is outstanding before the ack, so a diagnostic message there is
/// never taken as the answer.
#[tokio::test(start_paused = true)]
async fn diagnostic_message_before_the_ack_is_malformed() {
    let (mut session, mut gw) = link();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.expect(&REQUEST).await;
        gw.send(&reply(&TWO_FAULTS)).await;
        gw.send(&ack()).await;
    });
    assert_eq!(outcome.unwrap(), Outcome::Malformed);
    assert_disposed(&mut session, &mut gw).await;
}

/// Replay 2's stale answer: a second reply to a finished transaction must
/// not satisfy the next one.
#[tokio::test(start_paused = true)]
async fn leftover_reply_never_answers_the_next_request() {
    let (mut session, mut gw) = link();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.accept_request().await;
        gw.send(&reply(&ONE_FAULT)).await;
        // A second, different reply to the same request.
        gw.send(&reply(&TWO_FAULTS)).await;
    });
    let Outcome::Complete(report) = outcome.unwrap() else {
        panic!("expected a complete read");
    };
    assert_eq!(codes(&report), [(0x0A1B2C, 0x01)]);

    // The leftover reply is read before any ack, so the gateway only gets
    // as far as seeing the request.
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), gw.expect(&REQUEST));
    assert_eq!(outcome.unwrap(), Outcome::Malformed);
    assert_disposed(&mut session, &mut gw).await;
}

#[tokio::test(start_paused = true)]
async fn cancelling_a_read_closes_the_connection() {
    // Cancelled while waiting for the response.
    let (mut session, mut gw) = link();
    let (cancelled, ()) = tokio::join!(
        timeout(Duration::from_secs(1), session.read_dtcs(DME, MASK)),
        gw.accept_request()
    );
    assert!(cancelled.is_err(), "the read was still waiting when it was dropped");
    assert_disposed(&mut session, &mut gw).await;

    // Cancelled while waiting for the ack.
    let (mut session, mut gw) = link();
    let (cancelled, ()) = tokio::join!(
        timeout(Duration::from_secs(1), session.read_dtcs(DME, MASK)),
        gw.expect(&REQUEST)
    );
    assert!(cancelled.is_err());
    assert_disposed(&mut session, &mut gw).await;
}

#[tokio::test(start_paused = true)]
async fn gateway_error_frame_is_an_error_and_closes() {
    // Instead of the ack.
    let (mut session, mut gw) = link();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.expect(&REQUEST).await;
        gw.send(&frame(0x0041, &[])).await;
    });
    assert!(matches!(outcome, Err(Error::Transport(hsfz::Error::Rejected(0x0041)))));
    assert_disposed(&mut session, &mut gw).await;

    // Instead of the response.
    let (mut session, mut gw) = link();
    let (outcome, ()) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.accept_request().await;
        gw.send(&frame(0x00FF, &[])).await;
    });
    assert!(matches!(outcome, Err(Error::Transport(hsfz::Error::Rejected(0x00FF)))));
    assert_disposed(&mut session, &mut gw).await;
}

#[tokio::test(start_paused = true)]
async fn gateway_hanging_up_is_an_error() {
    let (mut session, mut gw) = link();
    let (outcome, gw) = tokio::join!(session.read_dtcs(DME, MASK), async {
        gw.accept_request().await;
        // Half of a response, then the gateway closes the link.
        gw.send(&reply(&TWO_FAULTS)[..9]).await;
        drop(gw);
    });
    let () = gw;
    assert!(matches!(outcome, Err(Error::Transport(hsfz::Error::Io(_)))));
    assert!(session.is_disposed());
    assert!(matches!(
        session.read_dtcs(DME, MASK).await,
        Err(Error::Transport(hsfz::Error::Disposed))
    ));
}

// ---- Read-only probe (DR-0001 amendment A1) ----

const ZGW: u8 = 0x10;
const VIN_DID: u16 = 0xF190;

fn dtc_request(ecu: u8) -> Vec<u8> {
    frame(DIAG, &[TESTER, ecu, 0x19, 0x02, MASK])
}

fn did_request(ecu: u8, did: u16) -> Vec<u8> {
    let [hi, lo] = did.to_be_bytes();
    frame(DIAG, &[TESTER, ecu, 0x22, hi, lo])
}

impl Gateway {
    /// Expects `request` and acks it by echoing its payload.
    async fn accept(&mut self, request: &[u8]) {
        self.expect(request).await;
        self.send(&frame(ACK, &request[6..])).await;
    }
}

/// `n` links prepared in advance. The returned connector hands them out in
/// order and fails once they are used up; the counter says how often it was
/// called.
#[allow(clippy::type_complexity)]
fn links(
    n: usize,
) -> (
    impl FnMut() -> std::future::Ready<Result<Session<DuplexStream>>>,
    Vec<Gateway>,
    std::rc::Rc<std::cell::Cell<usize>>,
) {
    let (mut sessions, gateways): (std::collections::VecDeque<_>, Vec<_>) = (0..n).map(|_| link()).unzip();
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let counter = calls.clone();
    let connect = move || {
        counter.set(counter.get() + 1);
        std::future::ready(sessions.pop_front().ok_or_else(|| {
            Error::Transport(hsfz::Error::Io(std::io::Error::other("no vehicle on the link")))
        }))
    };
    (connect, gateways, calls)
}

#[tokio::test(start_paused = true)]
async fn probe_asks_each_address_in_turn() {
    let (connect, mut gws, calls) = links(2);
    let start = Instant::now();
    let mut progress = Vec::new();
    let on_probe = |p: &Probe| progress.push(p.ecu);
    let (report, ()) = tokio::join!(probe(connect, &[ZGW, DME, EGS, 0x29], MASK, on_probe), async {
        // First connection: the gateway answers, the DME stays silent.
        let gw = &mut gws[0];
        gw.accept(&dtc_request(ZGW)).await;
        gw.send(&diag(ZGW, TESTER, &ONE_FAULT)).await;
        gw.accept(&dtc_request(DME)).await;
        gw.expect_closed().await;
        // Second connection: the EGS refuses, 0x29 has no faults.
        let gw = &mut gws[1];
        gw.accept(&dtc_request(EGS)).await;
        gw.send(&diag(EGS, TESTER, &[0x7F, 0x19, 0x11])).await;
        gw.accept(&dtc_request(0x29)).await;
        gw.send(&diag(0x29, TESTER, &[0x59, 0x02, 0xFF])).await;
    });
    let report = report.unwrap();
    assert!(report.stopped.is_none());
    let seen: Vec<_> = report
        .probes
        .iter()
        .map(|p| {
            let outcome = match p.faults.as_ref().unwrap() {
                Outcome::Complete(r) => Outcome::Complete(codes(r)),
                Outcome::Negative(nrc) => Outcome::Negative(*nrc),
                Outcome::PendingExpired => Outcome::PendingExpired,
                Outcome::NoResponse => Outcome::NoResponse,
                Outcome::Malformed => Outcome::Malformed,
            };
            (p.ecu, outcome)
        })
        .collect();
    assert_eq!(
        seen,
        [
            (ZGW, Outcome::Complete(vec![(0x0A1B2C, 0x01)])),
            (DME, Outcome::NoResponse),
            (EGS, Outcome::Negative(0x11)),
            (0x29, Outcome::Complete(vec![])),
        ]
    );
    assert_eq!(progress, [ZGW, DME, EGS, 0x29]);
    // One connection to start with and one after the silent address.
    assert_eq!(calls.get(), 2);
    // Only the silent address cost its timeout.
    assert_eq!(start.elapsed(), Duration::from_secs(2));
}

#[tokio::test(start_paused = true)]
async fn probe_refuses_functional_and_tester_addresses_before_any_io() {
    for bad in [0xDF, TESTER] {
        let (connect, mut gws, calls) = links(1);
        let result = probe(connect, &[ZGW, bad, DME], MASK, |_| {}).await;
        assert!(matches!(result, Err(Error::AddressNotAllowed(a)) if a == bad));
        assert_eq!(calls.get(), 0, "no connection was opened");
        // The unused link was dropped with the connector: nothing was sent.
        gws[0].expect_closed().await;
    }
}

#[tokio::test(start_paused = true)]
async fn probe_stops_when_it_cannot_reconnect() {
    let (connect, mut gws, calls) = links(1);
    let (report, ()) = tokio::join!(probe(connect, &[ZGW, DME, EGS], MASK, |_| {}), async {
        gws[0].accept(&dtc_request(ZGW)).await;
        // Silence: the connection is closed, and there is no second link.
    });
    let report = report.unwrap();
    assert_eq!(report.probes.len(), 1, "the addresses after the failure were never asked");
    assert_eq!(report.probes[0].ecu, ZGW);
    assert!(matches!(report.probes[0].faults, Ok(Outcome::NoResponse)));
    assert!(matches!(report.stopped, Some(Error::Transport(hsfz::Error::Io(_)))));
    assert_eq!(calls.get(), 2);
}

#[tokio::test(start_paused = true)]
async fn probe_records_a_gateway_rejection_and_goes_on() {
    let (connect, mut gws, calls) = links(2);
    let (report, ()) = tokio::join!(probe(connect, &[0x77, DME], MASK, |_| {}), async {
        gws[0].expect(&dtc_request(0x77)).await;
        gws[0].send(&frame(0x0042, &[])).await;
        gws[1].accept(&dtc_request(DME)).await;
        gws[1].send(&reply(&ONE_FAULT)).await;
    });
    let report = report.unwrap();
    assert!(matches!(
        report.probes[0].faults,
        Err(Error::Transport(hsfz::Error::Rejected(0x0042)))
    ));
    assert!(matches!(report.probes[1].faults, Ok(Outcome::Complete(_))));
    assert!(report.stopped.is_none());
    assert_eq!(calls.get(), 2);
}

#[tokio::test(start_paused = true)]
async fn read_data_returns_the_bytes_as_sent() {
    let (mut session, mut gw) = link();
    let (outcome, ()) = tokio::join!(session.read_data(DME, VIN_DID), async {
        gw.accept(&did_request(DME, VIN_DID)).await;
        gw.send(&reply(&[0x62, 0xF1, 0x90, 0x00, 0xFF, 0x41])).await;
    });
    assert_eq!(outcome.unwrap(), Outcome::Complete(vec![0x00, 0xFF, 0x41]));
    assert!(!session.is_disposed());

    // A refusal keeps the connection, and the next read uses it.
    let (outcome, ()) = tokio::join!(session.read_data(DME, 0x1234), async {
        gw.accept(&did_request(DME, 0x1234)).await;
        gw.send(&reply(&[0x7F, 0x22, 0x31])).await;
    });
    assert_eq!(outcome.unwrap(), Outcome::Negative(0x31));
    assert!(!session.is_disposed());
}

#[tokio::test(start_paused = true)]
async fn read_data_rejects_replies_that_are_not_its_answer() {
    let cases: [(&str, &[u8]); 5] = [
        ("another identifier echoed", &[0x62, 0xF1, 0x8C, 0x41]),
        ("identifier cut short", &[0x62, 0xF1]),
        ("no data after the identifier", &[0x62, 0xF1, 0x90]),
        ("positive SID of another service", &[0x59, 0xF1, 0x90, 0x41]),
        ("negative response to another service", &[0x7F, 0x19, 0x31]),
    ];
    for (what, data) in cases {
        let (mut session, mut gw) = link();
        let (outcome, ()) = tokio::join!(session.read_data(DME, VIN_DID), async {
            gw.accept(&did_request(DME, VIN_DID)).await;
            gw.send(&reply(data)).await;
        });
        assert_eq!(outcome.unwrap(), Outcome::Malformed, "{what}");
        assert!(session.is_disposed(), "{what}");
    }
}

/// The read list is enforced below every operation: a request for any other
/// service is refused before a byte is written.
#[tokio::test(start_paused = true)]
async fn only_read_services_are_ever_sent() {
    assert_eq!(READ_SERVICES, [0x19, 0x22]);
    let requests: [&[u8]; 6] = [
        &[0x14, 0xFF, 0xFF, 0xFF], // clear fault memory
        &[0x2E, 0xF1, 0x90, 0x00], // write data by identifier
        &[0x31, 0x01, 0x00, 0x00], // start a routine
        &[0x11, 0x01],             // reset
        &[0x10, 0x03],             // change session
        &[0x3E, 0x00],             // tester present
    ];
    for request in requests {
        let (mut session, mut gw) = link();
        let result = session.transact(DME, request, |body| Ok(body.to_vec())).await;
        assert!(matches!(result, Err(Error::NotARead(sid)) if sid == request[0]));
        // The connection is untouched and nothing went out on it.
        assert!(!session.is_disposed());
        drop(session);
        gw.expect_closed().await;
    }
}

#[tokio::test(start_paused = true)]
async fn single_reads_refuse_functional_and_tester_addresses() {
    for bad in [0xDF, TESTER] {
        let (mut session, mut gw) = link();
        assert!(matches!(session.read_dtcs(bad, MASK).await, Err(Error::AddressNotAllowed(a)) if a == bad));
        assert!(matches!(session.read_data(bad, VIN_DID).await, Err(Error::AddressNotAllowed(a)) if a == bad));
        drop(session);
        gw.expect_closed().await;
    }
}
