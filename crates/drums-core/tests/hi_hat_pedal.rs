use drums_core::mapping::{
    CcRange, DrumComponent, DrumKitConfiguration, DrumMapping, HiHatPedalMode, MappingEventKind,
    MidiInputResolver,
};
use drums_core::midi::{MidiEvent, MidiParser};
use drums_core::{
    ActualHit, MatchingConfig, MidiFile, MidiTiming, MusicalTransport, PracticeResultKind,
    PracticeSession, SetupWizard, SetupWizardState,
};

#[test]
fn imported_lessons_distinguish_open_closed_and_foot_and_score_them() {
    let track = [
        0, 0x99, 42, 80, 0x83, 0x60, 0x99, 46, 80, 0x83, 0x60, 0x99, 44, 80, 0, 0xff, 0x2f, 0,
    ];
    let mut bytes = b"MThd\0\0\0\x06\0\0\0\x01\x01\xe0MTrk".to_vec();
    bytes.extend_from_slice(&(track.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&track);
    let file = MidiFile::parse("hi-hat.mid", &bytes).unwrap();
    let config = kit();
    let expected = file.expected_notes_for_track(0, Some(&config));
    assert_eq!(
        expected
            .iter()
            .map(|note| note.target_component)
            .collect::<Vec<_>>(),
        vec![
            Some(DrumComponent::HiHat),
            Some(DrumComponent::HiHatOpen),
            Some(DrumComponent::HiHatPedal)
        ]
    );
    let transport = MusicalTransport::new(MidiTiming::TicksPerQuarter(480), &[], &[], 2000);
    let mut session = PracticeSession::new(expected, MatchingConfig::default());
    let mut resolver = MidiInputResolver::default();
    for (time, status, number, value, tick) in [
        (1, 0xb9, 4, 120, 0),
        (2, 0x99, 46, 81, 0),
        (3, 0xb9, 4, 0, 480),
        (4, 0x99, 26, 62, 480),
        (5, 0x99, 44, 55, 960),
    ] {
        let ev = event(time, status, number, value);
        let component = resolver.observe(&ev, &config);
        session.update_hi_hat_pedal_state(&ev, &config);
        if let Some(mut hit) = ActualHit::from_resolved_midi_event_observed_at(
            &ev,
            component,
            &transport,
            ev.timestamp_micros,
        ) {
            hit.transport_tick = tick;
            assert_eq!(
                session.record_hit(hit, &transport).kind,
                PracticeResultKind::CorrectHit
            );
        }
    }
    assert_eq!(session.results().len(), 3); // controller messages never became hits
    let mut wrong = PracticeSession::new(
        file.expected_notes_for_track(0, Some(&config)),
        MatchingConfig::default(),
    );
    let wrong_event = event(6, 0x99, 46, 80);
    let hit = ActualHit::from_resolved_midi_event_observed_at(
        &wrong_event,
        resolver.observe(&wrong_event, &config),
        &transport,
        6,
    )
    .unwrap();
    assert_eq!(
        wrong.record_hit(hit, &transport).kind,
        PracticeResultKind::WrongDrum
    );
    let mut legacy = config.clone();
    legacy.hi_hat_pedal = None;
    assert!(file
        .expected_notes_for_track(0, Some(&legacy))
        .iter()
        .all(|note| note.drum_component == Some(DrumComponent::HiHat)));
}

#[test]
fn wizard_learns_shared_stick_inputs_once_and_reviews_pedal_states() {
    let mut wizard = SetupWizard::with_existing(kit());
    wizard.start();
    wizard.skip();
    wizard.skip();
    assert_eq!(
        wizard.state(),
        SetupWizardState::Detecting(DrumComponent::HiHat)
    );
    wizard.handle_event(&event(100_000, 0x99, 46, 80));
    wizard.accept();
    assert_eq!(
        wizard.state(),
        SetupWizardState::Detecting(DrumComponent::HiHatPedal)
    );
    assert!(wizard.back());
    assert_eq!(
        wizard.state(),
        SetupWizardState::Detecting(DrumComponent::HiHat)
    );
    wizard.skip();
    while wizard.state() != SetupWizardState::Completed {
        wizard.skip();
    }
    let config = wizard.configuration("V51 Generic Mode").unwrap();
    assert!(config.validate().is_ok());
    wizard.handle_event(&event(200_000, 0xb9, 4, 120));
    wizard.handle_event(&event(200_001, 0x99, 46, 80));
    assert_eq!(wizard.test_component(), Some(DrumComponent::HiHat));
    wizard.handle_event(&event(200_002, 0xb9, 4, 0));
    wizard.handle_event(&event(200_003, 0x99, 26, 80));
    assert_eq!(wizard.test_component(), Some(DrumComponent::HiHatOpen));
}

fn kit() -> DrumKitConfiguration {
    let mut config = DrumKitConfiguration::new("V51 Generic Mode");
    config.hi_hat_pedal = Some(HiHatPedalMode::default());
    for (number, component, kind) in [
        (26, DrumComponent::HiHat, MappingEventKind::Note),
        (46, DrumComponent::HiHat, MappingEventKind::Note),
        (44, DrumComponent::HiHatPedal, MappingEventKind::Note),
        (
            4,
            DrumComponent::HiHatPedalPosition,
            MappingEventKind::ControlChange,
        ),
    ] {
        config
            .add_input(DrumMapping {
                component,
                event_kind: kind,
                midi_note: number,
                channel: 10,
                articulation: None,
                cc_range: (kind == MappingEventKind::ControlChange).then(|| CcRange::new(0, 127)),
            })
            .unwrap();
    }
    config
}

fn event(time: u64, status: u8, number: u8, value: u8) -> MidiEvent {
    MidiParser::new()
        .push_bytes(time, &[status, number, value])
        .remove(0)
}

#[test]
fn shared_zones_follow_pedal_without_reassigning_physical_inputs() {
    let config = kit();
    let mut resolver = MidiInputResolver::default();
    assert_eq!(resolver.observe(&event(0, 0x99, 46, 81), &config), None);
    for value in [116, 118, 120] {
        resolver.observe(&event(1, 0xb9, 4, value), &config);
        for note in [26, 46] {
            assert_eq!(
                resolver.observe(&event(2, 0x99, note, 81), &config),
                Some(DrumComponent::HiHat)
            );
        }
    }
    resolver.observe(&event(3, 0xb9, 4, 0), &config);
    for note in [26, 46] {
        assert_eq!(
            resolver.observe(&event(4, 0x99, note, 62), &config),
            Some(DrumComponent::HiHatOpen)
        );
    }
    assert_eq!(
        resolver.observe(&event(5, 0x99, 44, 55), &config),
        Some(DrumComponent::HiHatPedal)
    );
    assert_eq!(config.mappings.len(), 4);
    assert!(config.supports_component(DrumComponent::HiHatOpen));
}

#[test]
fn isolates_channels_controllers_and_resets_on_reconnect() {
    let config = kit();
    let mut resolver = MidiInputResolver::default();
    resolver.observe(&event(1, 0xb0, 4, 120), &config);
    resolver.observe(&event(1, 0xb9, 88, 120), &config);
    assert_eq!(resolver.observe(&event(2, 0x99, 46, 80), &config), None);
    resolver.observe(&event(3, 0xb9, 4, 120), &config);
    resolver.observe(&event(2, 0xb9, 4, 0), &config); // older CC cannot rewind the pedal
    assert_eq!(resolver.resolve(&event(2, 0x99, 46, 80), &config), None); // no future pedal state
    assert_eq!(
        resolver.observe(&event(4, 0x99, 46, 80), &config),
        Some(DrumComponent::HiHat)
    );
    assert_eq!(resolver.observe(&event(5, 0x99, 46, 0), &config), None);
    assert_eq!(resolver.observe(&event(5, 0x89, 46, 80), &config), None);
    resolver.reset();
    assert_eq!(resolver.observe(&event(6, 0x99, 46, 80), &config), None);
}

#[test]
fn threshold_direction_and_legacy_profiles_are_explicit() {
    let mut config = kit();
    let mut resolver = MidiInputResolver::default();
    for (value, component) in [(89, DrumComponent::HiHatOpen), (90, DrumComponent::HiHat)] {
        resolver.observe(&event(1, 0xb9, 4, value), &config);
        assert_eq!(
            resolver.observe(&event(2, 0x99, 46, 80), &config),
            Some(component)
        );
    }
    config.hi_hat_pedal = Some(HiHatPedalMode {
        closed_threshold: 40,
        high_values_closed: false,
    });
    resolver.observe(&event(3, 0xb9, 4, 0), &config);
    assert_eq!(
        resolver.observe(&event(4, 0x99, 46, 80), &config),
        Some(DrumComponent::HiHat)
    );
    let saved = toml::to_string(&config).unwrap();
    let loaded: DrumKitConfiguration = toml::from_str(&saved).unwrap();
    assert_eq!(loaded, config);
    config.hi_hat_pedal = None;
    resolver.reset();
    assert_eq!(
        resolver.observe(&event(5, 0x99, 46, 80), &config),
        Some(DrumComponent::HiHat)
    );
    let legacy = toml::to_string(&config).unwrap();
    assert!(!legacy.contains("hi_hat_pedal"));
    assert!(toml::from_str::<DrumKitConfiguration>(&legacy)
        .unwrap()
        .hi_hat_pedal
        .is_none());
}

#[test]
fn replay_measured_v51_closed_open_and_foot_captures() {
    for (trace, stick_component, expected_sticks, expected_feet) in [
        (
            include_str!("fixtures/v51-closed.csv"),
            DrumComponent::HiHat,
            15,
            1,
        ),
        (
            include_str!("fixtures/v51-open.csv"),
            DrumComponent::HiHatOpen,
            16,
            0,
        ),
        (
            include_str!("fixtures/v51-foot.csv"),
            DrumComponent::HiHat,
            0,
            9,
        ),
    ] {
        let config = kit();
        let mut resolver = MidiInputResolver::default();
        let mut sticks = 0;
        let mut feet = 0;
        for line in trace.lines().filter(|line| !line.starts_with('#')) {
            let values: Vec<u64> = line.split(',').map(|s| s.parse().unwrap()).collect();
            let ev = event(
                values[0] * 1000,
                values[1] as u8,
                values[2] as u8,
                values[3] as u8,
            );
            let component = resolver.observe(&ev, &config);
            if values[1] == 0x99 && values[3] > 0 {
                if values[2] == 44 {
                    feet += 1;
                    assert_eq!(component, Some(DrumComponent::HiHatPedal));
                } else if matches!(values[2], 26 | 46) {
                    sticks += 1;
                    assert_eq!(component, Some(stick_component));
                }
            }
        }
        assert_eq!((sticks, feet), (expected_sticks, expected_feet));
    }
}

#[test]
fn shared_mode_requires_one_pedal_controller_on_each_stick_channel() {
    let valid = kit();
    assert!(valid.validate().is_ok());
    let mut missing = valid.clone();
    missing.remove(DrumComponent::HiHatPedalPosition);
    assert!(missing.validate().is_err());
    let mut wrong_channel = valid.clone();
    wrong_channel.mappings.last_mut().unwrap().channel = 1;
    assert!(wrong_channel.validate().is_err());
    let mut ambiguous = valid.clone();
    let mut second_controller = *ambiguous.mappings.last().unwrap();
    second_controller.midi_note = 19;
    ambiguous.add_input(second_controller).unwrap();
    assert!(ambiguous.validate().is_err());
    let mut threshold = valid;
    threshold.hi_hat_pedal.as_mut().unwrap().closed_threshold = 128;
    assert!(threshold.validate().is_err());
    missing.hi_hat_pedal = None;
    assert!(missing.validate().is_ok());
}

#[test]
fn resolved_hits_keep_callback_timing_and_never_score_controller_updates() {
    let config = kit();
    let mut resolver = MidiInputResolver::default();
    let mut transport = MusicalTransport::new(MidiTiming::TicksPerQuarter(480), &[], &[], 2000);
    transport.play();
    transport.advance_by(std::time::Duration::from_secs(1));
    let cc = event(800_000, 0xb9, 4, 0);
    let component = resolver.observe(&cc, &config);
    assert!(
        ActualHit::from_resolved_midi_event_observed_at(&cc, component, &transport, 1_000_000,)
            .is_none()
    );
    let note = event(900_000, 0x99, 46, 73);
    let hit = ActualHit::from_resolved_midi_event_observed_at(
        &note,
        resolver.observe(&note, &config),
        &transport,
        1_000_000,
    )
    .unwrap();
    assert_eq!(hit.drum_component, Some(DrumComponent::HiHatOpen));
    assert_eq!(hit.input_timestamp_micros, 900_000);
    assert_eq!(hit.received_midi_note, 46);
    assert_eq!(hit.velocity, 73);
    assert_eq!(hit.channel, 9);
    assert!((hit.elapsed_seconds - 0.9).abs() < 1e-9);
    assert_eq!(hit.transport_tick, transport.effective_tick_at_seconds(0.9));
}
