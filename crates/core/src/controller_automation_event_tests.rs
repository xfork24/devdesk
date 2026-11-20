use super::*;

#[test]
fn automatic_notices_coalesce_only_owned_normal_human_progress() {
    let hint = json!({"controller_id":"manager","epoch":2,"prepare_human_requests":true});
    for kind in ["human.confirmed", "decision.changed"] {
        let event = |status: &str, hint: Value| {
            json!({"id":1,"detail":json!({"root_instance_id":"root","change":{
            "kind":kind,"status":status,"delegated_progress":hint}}).to_string()})
        };
        for status in ["pending", "approved"] {
            let raw = event(status, hint.clone());
            let (items, cursor) =
                native_event_items(std::slice::from_ref(&raw), "root", 0, Some(("manager", 2)))
                    .unwrap();
            assert!(items.is_empty());
            assert_eq!(cursor, 1);
            for owner in [None, Some(("foreign", 2)), Some(("manager", 3))] {
                assert_eq!(
                    native_event_items(std::slice::from_ref(&raw), "root", 0, owner)
                        .unwrap()
                        .0
                        .len(),
                    1
                );
            }
        }
    }
    for change in [
        json!({"kind":"decision.changed","status":"rejected","delegated_progress":hint}),
        json!({"kind":"human.confirmed"}),
        json!({"kind":"plan.change"}),
        json!({"kind":"execution.reconciled","status":"unresolved"}),
    ] {
        let raw =
            json!({"id":1,"detail":json!({"root_instance_id":"root","change":change}).to_string()});
        assert_eq!(
            native_event_items(&[raw], "root", 0, Some(("manager", 2)))
                .unwrap()
                .0
                .len(),
            1
        );
    }
}

#[test]
fn only_the_owned_proven_normal_stop_is_coalesced_before_output_admission() {
    for normal in [true, false] {
        let hint = json!({"controller_id":"manager","epoch":2,"prepare_human_requests":true,"normal_native_stop":normal});
        let raw = json!({"id":1,"time":100,"detail":json!({"root_instance_id":"root","change":{"kind":"execution.stopped","turn_id":"turn","delegated_progress":hint}}).to_string()});
        for owner in [
            Some(("manager", 2)),
            Some(("manager", 3)),
            Some(("foreign", 2)),
            None,
        ] {
            let (items, cursor) =
                native_event_items_at(std::slice::from_ref(&raw), "root", 0, owner, 10_000)
                    .unwrap();
            assert_eq!(items.is_empty(), normal && owner == Some(("manager", 2)));
            if items.is_empty() {
                assert_eq!(cursor, 1);
            }
        }
    }
}
