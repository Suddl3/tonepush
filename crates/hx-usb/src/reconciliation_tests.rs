// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Suddl3
//! HX Unbound regressions for device changes mixed with editor work.
//! All traffic here is synthetic. These tests never enumerate USB devices.
use super::*;

struct DuringWrite {
    cable: fake::Cable,
    pedal: std::sync::Arc<std::sync::Mutex<fake::Pedal>>,
    injected: bool,
}

impl Wire for DuringWrite {
    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.cable.send(bytes)?;
        let mut pedal = self.pedal.lock().unwrap();
        if !self.injected {
            if let Some((_, txn, rpc::op::WRITE_PRESET, _)) = pedal.requests.last() {
                let txn = *txn;
                self.injected = true;
                pedal.notify(30, Value::Int(74));
                pedal.notify(20, hx_proto::msgmap! {
                    rpc::key::TXN => Value::Int(txn),
                    rpc::key::STATUS => Value::Int(0),
                });
                pedal.notify(49, Value::Bool(false));
            }
        }
        Ok(())
    }

    fn recv(&mut self, timeout: Duration) -> Result<Vec<u8>> {
        self.cable.recv(timeout)
    }
}

#[test]
fn notifications_survive_before_during_and_after_deferred_completion() {
    let pedal = fake::Pedal::new();
    let wire = DuringWrite {
        cable: fake::Cable(pedal.clone()),
        pedal: pedal.clone(),
        injected: false,
    };
    let mut session = Session::replaying(Box::new(wire), hx_proto::HELIX_FLOOR).unwrap();
    let preset = session.read_preset().unwrap();
    session.poll_notifications().unwrap();
    {
        let mut device = pedal.lock().unwrap();
        device.defers_writes = true;
        device.notify(30, Value::Int(69));
    }
    session.write_preset(&preset).unwrap();
    let mut notes = session.poll_notifications().unwrap();
    notes.extend(session.poll_notifications().unwrap());
    assert_eq!(
        notes.into_iter().filter(|(event, _)| *event != 20).collect::<Vec<_>>(),
        vec![(30, Value::UInt(69)), (30, Value::UInt(74)), (49, Value::Bool(false))]
    );
    assert!(session.poll_notifications().unwrap().is_empty(), "each event appears once");
}


#[test]
fn a_large_burst_progresses_across_an_ack_window() {
    let pedal = fake::Pedal::new();
    let mut session = fake::session(&pedal);
    session.poll_notifications().unwrap();
    pedal.lock().unwrap().events_window = Some(4096);
    for value in 0..300 { pedal.lock().unwrap().notify(30, Value::Int(value)); }
    let mut notes = session.poll_notifications().unwrap();
    session.keepalive().unwrap();
    for _ in 0..10 { notes.extend(session.poll_notifications().unwrap()); }
    assert_eq!(notes.iter().map(|(e, v)| (*e, v.as_i64())).collect::<Vec<_>>(),
               (0..300).map(|v| (30, Some(v))).collect::<Vec<_>>());
}
