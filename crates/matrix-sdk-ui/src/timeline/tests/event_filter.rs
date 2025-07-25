// Copyright 2023 Kévin Commaille
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::sync::Arc;

use assert_matches2::assert_let;
use eyeball_im::VectorDiff;
use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk_test::{ALICE, BOB, async_test, sync_timeline_event};
use ruma::events::{
    AnySyncTimelineEvent, TimelineEventType,
    room::{
        member::MembershipState,
        message::{MessageType, RedactedRoomMessageEventContent},
    },
};
use stream_assert::assert_next_matches;

use super::TestTimeline;
use crate::timeline::{
    AnyOtherFullStateEventContent, MsgLikeContent, MsgLikeKind, TimelineEventTypeFilter,
    TimelineItem, TimelineItemContent, TimelineItemKind, controller::TimelineSettings,
    tests::TestTimelineBuilder,
};

#[async_test]
async fn test_default_filter() {
    let timeline = TestTimeline::new();
    let mut stream = timeline.subscribe().await;

    let f = &timeline.factory;

    // Test edits work.
    timeline.handle_live_event(f.text_msg("The first message").sender(&ALICE)).await;

    let item = assert_next_matches!(stream, VectorDiff::PushBack { value } => value);
    let _date_divider = assert_next_matches!(stream, VectorDiff::PushFront { value } => value);
    let first_event_id = item.as_event().unwrap().event_id().unwrap();

    timeline
        .handle_live_event(
            f.text_msg(" * The _edited_ first message")
                .sender(&ALICE)
                .edit(first_event_id, MessageType::text_plain("The _edited_ first message").into()),
        )
        .await;

    // The edit was applied.
    let item = assert_next_matches!(stream, VectorDiff::Set { index: 1, value } => value);
    assert_let!(Some(message) = item.as_event().unwrap().content().as_message());
    assert_let!(MessageType::Text(text) = message.msgtype());
    assert_eq!(text.body, "The _edited_ first message");

    // TODO: After adding raw timeline items, check for one here.

    // Test redactions work.
    timeline.handle_live_event(f.text_msg("The second message").sender(&ALICE)).await;
    let item = assert_next_matches!(stream, VectorDiff::PushBack { value } => value);
    let second_event_id = item.as_event().unwrap().event_id().unwrap();

    timeline.handle_live_event(f.redaction(second_event_id).sender(&BOB)).await;
    let item = assert_next_matches!(stream, VectorDiff::Set { index: 2, value } => value);
    assert!(item.as_event().unwrap().content().is_redacted());

    // TODO: After adding raw timeline items, check for one here.

    // Test reactions work.
    timeline.handle_live_event(f.text_msg("The third message").sender(&ALICE)).await;
    let item = assert_next_matches!(stream, VectorDiff::PushBack { value } => value);
    let third_event_id = item.as_event().unwrap().event_id().unwrap();

    timeline.handle_live_event(f.reaction(third_event_id, "+1").sender(&BOB)).await;
    timeline.handle_live_event(f.redaction(second_event_id).sender(&BOB)).await;
    let item = assert_next_matches!(stream, VectorDiff::Set { index: 3, value } => value);
    assert_eq!(
        item.as_event().unwrap().content().reactions().cloned().unwrap_or_default().len(),
        1
    );

    // TODO: After adding raw timeline items, check for one here.

    assert_eq!(timeline.controller.items().await.len(), 4);
}

#[async_test]
async fn test_filter_always_false() {
    let timeline = TestTimelineBuilder::new()
        .settings(TimelineSettings { event_filter: Arc::new(|_, _| false), ..Default::default() })
        .build();

    let f = &timeline.factory;
    timeline.handle_live_event(f.text_msg("The first message").sender(&ALICE)).await;

    timeline.handle_live_event(f.redacted(&ALICE, RedactedRoomMessageEventContent::new())).await;

    timeline.handle_live_event(f.member(&ALICE).membership(MembershipState::Join)).await;

    timeline.handle_live_event(f.room_name("Alice's room").sender(&ALICE)).await;

    assert_eq!(timeline.controller.items().await.len(), 0);
}

#[async_test]
async fn test_custom_filter() {
    // Filter out all state events.
    let timeline = TestTimelineBuilder::new()
        .settings(TimelineSettings {
            event_filter: Arc::new(|ev, _| matches!(ev, AnySyncTimelineEvent::MessageLike(_))),
            ..Default::default()
        })
        .build();
    let mut stream = timeline.subscribe().await;

    let f = &timeline.factory;
    timeline.handle_live_event(f.text_msg("The first message").sender(&ALICE)).await;
    let _item = assert_next_matches!(stream, VectorDiff::PushBack { value } => value);
    let _date_divider = assert_next_matches!(stream, VectorDiff::PushFront { value } => value);

    timeline.handle_live_event(f.redacted(&ALICE, RedactedRoomMessageEventContent::new())).await;
    let _item = assert_next_matches!(stream, VectorDiff::PushBack { value } => value);

    timeline.handle_live_event(f.member(&ALICE).membership(MembershipState::Join)).await;

    timeline.handle_live_event(f.room_name("Alice's room").sender(&ALICE)).await;

    assert_eq!(timeline.controller.items().await.len(), 3);
}

#[async_test]
async fn test_hide_failed_to_parse() {
    let timeline = TestTimelineBuilder::new()
        .settings(TimelineSettings { add_failed_to_parse: false, ..Default::default() })
        .build();

    // m.room.message events must have a msgtype and body in content, so this
    // event with an empty content object should fail to deserialize.
    timeline
        .handle_live_event(TimelineEvent::from_plaintext(sync_timeline_event!({
            "content": {},
            "event_id": "$eeG0HA0FAZ37wP8kXlNkxx3I",
            "origin_server_ts": 10,
            "sender": "@alice:example.org",
            "type": "m.room.message",
        })))
        .await;

    // Similar to above, the m.room.member state event must also not have an
    // empty content object.
    timeline
        .handle_live_event(TimelineEvent::from_plaintext(sync_timeline_event!({
            "content": {},
            "event_id": "$d5G0HA0FAZ37wP8kXlNkxx3I",
            "origin_server_ts": 2179,
            "sender": "@alice:example.org",
            "type": "m.room.member",
            "state_key": "@alice:example.org",
        })))
        .await;

    assert_eq!(timeline.controller.items().await.len(), 0);
}

#[async_test]
async fn test_event_type_filter_include_only_room_names() {
    // Only return room name events
    let event_filter = TimelineEventTypeFilter::Include(vec![TimelineEventType::RoomName]);

    let timeline = TestTimelineBuilder::new()
        .settings(TimelineSettings {
            event_filter: Arc::new(move |event, _| event_filter.filter(event)),
            ..Default::default()
        })
        .build();
    let f = &timeline.factory;

    // Add a non-encrypted message event
    timeline.handle_live_event(f.text_msg("The first message").sender(&ALICE)).await;
    // Add a couple of room name events
    timeline.handle_live_event(f.room_name("A new room name").sender(&ALICE)).await;
    timeline.handle_live_event(f.room_name("A new room name (again)").sender(&ALICE)).await;
    // And a different state event
    timeline.handle_live_event(f.room_topic("A new room topic").sender(&ALICE)).await;

    // The timeline should contain only the room name events
    let event_items: Vec<Arc<TimelineItem>> = timeline.get_event_items().await;
    let text_message_items_count = event_items.iter().filter(is_text_message_item).count();
    let room_name_items_count = event_items.iter().filter(is_room_name_item).count();
    let room_topic_items_count = event_items.iter().filter(is_room_topic_item).count();
    assert_eq!(event_items.len(), 2);
    assert_eq!(text_message_items_count, 0);
    assert_eq!(room_name_items_count, 2);
    assert_eq!(room_topic_items_count, 0);
}

#[async_test]
async fn test_event_type_filter_exclude_messages() {
    // Don't return any messages
    let event_filter = TimelineEventTypeFilter::Exclude(vec![TimelineEventType::RoomMessage]);

    let timeline = TestTimelineBuilder::new()
        .settings(TimelineSettings {
            event_filter: Arc::new(move |event, _| event_filter.filter(event)),
            ..Default::default()
        })
        .build();
    let f = &timeline.factory;

    // Add a message event
    timeline.handle_live_event(f.text_msg("The first message").sender(&ALICE)).await;
    // Add a couple of room name state events
    timeline.handle_live_event(f.room_name("A new room name").sender(&ALICE)).await;
    timeline.handle_live_event(f.room_name("A new room name (again)").sender(&ALICE)).await;
    // And a different state event
    timeline.handle_live_event(f.room_topic("A new room topic").sender(&ALICE)).await;

    // The timeline should contain everything except for the message event.
    let event_items: Vec<Arc<TimelineItem>> = timeline.get_event_items().await;
    let text_message_items_count = event_items.iter().filter(is_text_message_item).count();
    let room_name_items_count = event_items.iter().filter(is_room_name_item).count();
    let room_topic_items_count = event_items.iter().filter(is_room_topic_item).count();
    assert_eq!(event_items.len(), 3);
    assert_eq!(text_message_items_count, 0);
    assert_eq!(room_name_items_count, 2);
    assert_eq!(room_topic_items_count, 1);
}

impl TestTimeline {
    async fn get_event_items(&self) -> Vec<Arc<TimelineItem>> {
        self.controller
            .items()
            .await
            .into_iter()
            .filter(|i| matches!(i.kind, TimelineItemKind::Event(_)))
            .collect()
    }
}

fn is_text_message_item(item: &&Arc<TimelineItem>) -> bool {
    match item.kind() {
        TimelineItemKind::Event(event) => match &event.content {
            TimelineItemContent::MsgLike(MsgLikeContent {
                kind: MsgLikeKind::Message(message),
                ..
            }) => {
                matches!(message.msgtype, MessageType::Text(_))
            }
            _ => false,
        },
        _ => false,
    }
}

fn is_room_name_item(item: &&Arc<TimelineItem>) -> bool {
    match item.kind() {
        TimelineItemKind::Event(event) => match &event.content {
            TimelineItemContent::OtherState(state) => {
                matches!(state.content, AnyOtherFullStateEventContent::RoomName(_))
            }
            _ => false,
        },
        _ => false,
    }
}

fn is_room_topic_item(item: &&Arc<TimelineItem>) -> bool {
    match item.kind() {
        TimelineItemKind::Event(event) => match &event.content {
            TimelineItemContent::OtherState(state) => {
                matches!(state.content, AnyOtherFullStateEventContent::RoomTopic(_))
            }
            _ => false,
        },
        _ => false,
    }
}
