 Build an Open-Source MIDI Drum Practice Application
1. Project Overview
Build a professional, open-source desktop application designed specifically to help people learn and practice drums using electronic drum kits connected through MIDI/USB.
The application should allow a user to:
1.	Connect virtually any MIDI-compatible electronic drum kit.
2.	Automatically configure the kit through an interactive setup wizard.
3.	Detect which MIDI note corresponds to each physical drum component.
4.	Import MIDI drum tracks/files for songs.
5.	Display the drum notes on a visual timeline.
6.	Play the song while the user plays along on their physical MIDI drum kit.
7.	Compare the user's actual MIDI performance against the expected MIDI performance.
8.	Calculate timing accuracy in milliseconds.
9.	Detect hits, misses, wrong drum hits, early hits, late hits, and perfect hits.
10.	Provide an overall accuracy score at the end.
11.	Provide detailed timing statistics.
12.	Allow sections of songs to be looped repeatedly.
13.	Allow songs to be slowed down or sped up without changing pitch.
14.	Support backing tracks such as WAV/MP3.
15.	Provide a highly configurable metronome.
16.	Display musical counting such as 1 & 2 & 3 & 4 & synchronized with the song.
17.	Support different time signatures and subdivisions.
18.	Provide a count-in before playback.
19.	Eventually maintain a song/practice library and track progress.
The application should feel like a serious drum practice workstation, not primarily a rhythm game.
The central philosophy is:
Connect any MIDI drum kit → configure it → import a drum MIDI → practice → receive precise timing feedback.
________________________________________
2. Technology Requirements
Use Rust as the primary programming language.
The application must be designed as a native desktop application rather than a browser-based application.
The main reasons for using Rust are:
•	Low latency
•	Precise timing
•	Real-time MIDI processing
•	Reliable audio processing
•	Multithreading
•	Native desktop performance
•	Cross-platform support
•	Good long-term maintainability
•	Open-source friendliness
Use appropriate Rust libraries for:
MIDI
Use a reliable Rust MIDI library such as midir or an equivalent well-maintained solution.
The MIDI engine must support:
•	MIDI input device discovery
•	MIDI device selection
•	MIDI note-on events
•	MIDI note-off events
•	MIDI velocity
•	High-resolution timestamps
•	MIDI device reconnection
•	Multiple MIDI devices if practical
Audio
Use an appropriate low-latency Rust audio library such as cpal or another suitable solution.
The audio engine should eventually handle:
•	Metronome clicks
•	Count-in
•	Backing tracks
•	MIDI playback if required
•	Volume control
•	Low-latency audio output
GUI
Use a native Rust GUI framework such as egui/eframe, unless there is a strong technical reason to use another Rust-native framework.
The UI should be responsive and must not block the MIDI or audio processing threads.
________________________________________
3. Core Architecture
Separate the application into clear subsystems.
Recommended architecture:
                         ┌──────────────────────┐
                         │      Rust App        │
                         └──────────┬───────────┘
                                    │
              ┌─────────────────────┼─────────────────────┐
              ↓                     ↓                     ↓
        MIDI Engine            Audio Engine           GUI Engine
              │                     │                     │
              ↓                     ↓                     ↓
        E-Drum Input          Metronome              Timeline
        MIDI Mapping           Count-in              Beat Display
        MIDI Parser            Backing Track          Controls
              │                     │                     │
              └─────────────────────┼─────────────────────┘
                                    ↓
                             TRANSPORT CLOCK
                                    │
                                    ↓
                           PRACTICE ENGINE
                                    │
                    ┌───────────────┴───────────────┐
                    ↓                               ↓
              Expected Notes                   Player Notes
                    │                               │
                    └───────────────┬───────────────┘
                                    ↓
                              MATCH ENGINE
                                    │
                                    ↓
                            ACCURACY ENGINE
                                    │
                                    ↓
                           PRACTICE RESULTS
Critical architectural requirement
The metronome, MIDI timeline, song playback, visual beat counter, and accuracy engine must all be synchronized to the same master transport clock.
Do NOT create separate independent timers for:
•	Metronome
•	MIDI notes
•	GUI animation
•	Accuracy measurement
All timing-sensitive components should reference the same high-resolution transport position.
This is critical for accurate scoring and low latency.
________________________________________
4. MIDI Drum Setup Wizard
The first time a user connects a drum kit, provide an interactive configuration wizard.
The application should automatically detect available MIDI input devices.
Example:
MIDI DEVICES

NUX DM-1
USB MIDI Device
Generic MIDI Controller
The user selects their drum kit.
Then show:
DRUM KIT SETUP

We are going to identify each component.

You will be asked to hit one drum at a time.

[ Start Setup ]
Automatic mapping process
For every supported component:
•	Crash
•	Ride
•	Hi-Hat
•	Snare
•	Kick
•	High Tom
•	Mid Tom
•	Low Tom
Prompt:
Hit your SNARE once.
Wait for the next MIDI note-on event.
For example:
Received MIDI:
Note = 38
Velocity = 105

SNARE → MIDI NOTE 38
Then proceed:
✓ Snare configured

Next:

Hit your KICK once.
Continue until all components are mapped.
The application should support different MIDI note layouts because different electronic drum kits may use different MIDI notes.
Do NOT assume a universal mapping.
________________________________________
5. Manual MIDI Mapping
After automatic setup, provide a configuration screen.
Example:
DRUM MAPPING

CRASH      MIDI 49
RIDE       MIDI 51
HI-HAT     MIDI 42
SNARE      MIDI 38
KICK       MIDI 36
HI TOM     MIDI 48
MID TOM    MIDI 47
LOW TOM    MIDI 45

[ Edit Mapping ]
Allow the user to manually change:
•	MIDI note
•	MIDI channel if necessary
•	Velocity behavior if necessary
The mapping should be saved locally for that device.
If the same drum kit is reconnected later, the application should automatically restore its configuration.
________________________________________
6. Main Practice Screen
The primary interface should resemble the provided reference image.
The reference design has:
•	Dark background
•	Drum components listed vertically on the left
•	One horizontal lane for each drum component
•	A large central timeline
•	A vertical hit/playback line
•	Minimal, professional visual design
•	Colored drum labels/icons
•	Large usable workspace
Do NOT turn this into a Guitar Hero-style game.
It should look like a professional music practice application.
The main lanes should be:
CRASH
RIDE
HI-HAT
SNARE
KICK
HI TOM
MID TOM
LOW TOM
Each drum has its own horizontal lane.
Example:
┌──────────────┬───────────────────────────────────────────────┐
│ CRASH        │                                               │
├──────────────┤                                               │
│ RIDE         │                                               │
├──────────────┤                                               │
│ HI-HAT       │       ●                 ●                 ●   │
├──────────────┤                                               │
│ SNARE        │             ●                       ●         │
├──────────────┤                                               │
│ KICK         │       ●                       ●               │
├──────────────┤                                               │
│ HI TOM       │                                               │
├──────────────┤                                               │
│ MID TOM      │                                               │
├──────────────┤                                               │
│ LOW TOM      │                                               │
└──────────────┴───────────────────────────────────────────────┘
Use distinct but tasteful colors for each drum type.
________________________________________
7. Timeline
The timeline should show upcoming MIDI notes.
The user should be able to visually understand:
•	What drum to hit
•	When to hit it
•	Which notes are coming next
•	Where the current playback position is
Use a clear vertical hit line.
Example:
                       NOTES
                         ↓
             ●           ●
                    ●
       ●                    ●
────────────────────────────────
                 ↑
              HIT LINE
The timeline should scroll smoothly during playback.
The user should be able to zoom the timeline if practical.
________________________________________
8. MIDI File Import
Allow the user to import .mid / .midi files.
Possible interaction:
Drag MIDI file here

or

[ Import MIDI ]
After loading a MIDI file:
1.	Parse the MIDI file.
2.	Detect tracks.
3.	Identify MIDI notes.
4.	Map them to configured drum components.
5.	Extract tempo information.
6.	Extract time signature information if available.
7.	Extract note timing.
8.	Prepare the notes for playback/practice.
If the MIDI contains multiple tracks, allow the user to select which track is the drum track.
Example:
MIDI TRACKS

☑ Drums
☐ Bass
☐ Guitar
☐ Piano
Initially, prioritize drum MIDI tracks.
________________________________________
9. MIDI Note Representation
Internally, represent expected drum events with something similar to:
ExpectedNote {
    timestamp
    drum_type
    midi_note
    velocity
    measure
    beat
    subdivision
}
For example:
0.000 sec   Kick
0.000 sec   Hi-Hat
0.500 sec   Snare
0.500 sec   Hi-Hat
1.000 sec   Kick
1.000 sec   Hi-Hat
1.500 sec   Snare
1.500 sec   Hi-Hat
Do not depend exclusively on floating-point wall-clock timing for musical position.
Maintain musical information such as:
•	Measure
•	Beat
•	Tick
•	Tempo
•	Time signature
where available.
________________________________________
10. Master Transport Clock
Create a dedicated transport system.
The transport should expose:
current_position
current_beat
current_measure
current_tick
tempo
time_signature
playing
paused
loop_start
loop_end
playback_speed
Everything should reference this transport.
The transport drives:
•	MIDI note visualization
•	Metronome
•	Beat counter
•	Count-in
•	Backing track playback
•	Practice scoring
•	Looping
•	Playback speed
________________________________________
11. Metronome — Major Feature
The metronome is one of the most important features of the entire application.
Do NOT treat it as a small optional button.
It should be tightly integrated with the transport.
At the top of the screen, display a large musical counting indicator.
For example:
95 BPM     4/4     EIGHTH NOTES

        1     &     2     &     3     &     4     &
        ●           ●           ●           ●
As the song plays, the current beat/subdivision should be visually highlighted.
Example:
        1     &     2     &     3     &     4     &
              ▲
Then:
        1     &     2     &     3     &     4     &
                    ▲
Then:
        1     &     2     &     3     &     4     &
                          ▲
The visual indicator and audible metronome must remain synchronized.
________________________________________
12. Metronome Subdivisions
Allow the user to configure the counting system.
Quarter notes
1     2     3     4
●     ●     ●     ●
Eighth notes
1   &   2   &   3   &   4   &
●       ●       ●       ●
Sixteenth notes
1 e & a 2 e & a 3 e & a 4 e & a
●        ●        ●        ●
Triplets
1 trip let 2 trip let 3 trip let 4 trip let
●          ●          ●          ●
Support additional useful subdivisions later.
The user should be able to select:
SUBDIVISION

Quarter
Eighth
Sixteenth
Triplets
________________________________________
13. Configurable Time Signature
Support common time signatures:
4/4
3/4
6/8
5/4
7/8
Eventually allow custom time signatures.
The visual counting should automatically adapt.
For 4/4:
1 & 2 & 3 & 4 &
For 3/4:
1 & 2 & 3 &
For 6/8:
1 & a 2 & a
The counting system should reflect the selected subdivision and musical meter.
________________________________________
14. Metronome Accent
Allow configurable accents.
Possible modes:
Accent:
● Beat 1
● Every beat
● No accent
Beat 1 should be distinguishable from other beats through a different click sound and/or visual indicator.
________________________________________
15. BPM Control
Provide a prominent BPM control.
Example:
♩ = 95 BPM
Allow:
•	Manual BPM entry
•	Increase/decrease buttons
•	Slider if appropriate
•	Keyboard shortcuts if useful
BPM should be changeable during practice where technically safe.
________________________________________
16. Count-In
Before playback begins, optionally provide a count-in.
Example:
4
3
2
1
GO
Each number should correspond to actual metronome beats based on the selected tempo and time signature.
Do not implement the count-in as an unrelated arbitrary timer.
It must use the same transport timing system.
________________________________________
17. Playback Speed
Allow the user to practice at reduced speed.
Example:
100%
90%
80%
70%
60%
50%
The user should be able to practice a difficult section slowly and gradually increase speed.
The timing relationship between notes must remain correct.
For example:
Original:
120 BPM

Practice:
60 BPM
The MIDI note spacing should double while preserving musical relationships.
Backing tracks should ideally use time-stretching without changing pitch.
________________________________________
18. Song Looping
Allow the user to select a specific section.
Example:
00:42 ───────────────────── 01:05
       LOOP REGION
Controls:
[ Set Loop Start ]
[ Set Loop End ]
[ Enable Loop ]
Or allow the user to drag loop markers directly on the timeline.
Support:
•	Loop indefinitely
•	Loop a fixed number of times
•	Restart section automatically
The loop should preserve exact musical timing.
________________________________________
19. Backing Tracks
Allow users to load a backing track.
Support common formats such as:
•	WAV
•	MP3
•	FLAC if practical
•	Other appropriate formats
The backing track should be independently controllable.
Example:
BACKING TRACK     ─────●────
METRONOME         ─────────●
DRUM AUDIO        ────●─────
The backing track should synchronize to the transport.
Eventually support:
•	Offset adjustment
•	Volume control
•	Mute
•	Solo
•	Optional tempo synchronization
________________________________________
20. Player MIDI Input
During practice, listen continuously to the connected MIDI drum kit.
Every MIDI note-on should be timestamped with the highest practical precision.
Capture:
timestamp
midi_note
drum_type
velocity
channel
Example:
10.473 s
MIDI 38
Snare
Velocity 105
________________________________________
21. Accuracy Engine
This is one of the most important parts of the application.
For every expected note, determine whether the player performed the correct drum at approximately the correct time.
Example:
Expected:

Snare @ 10.500 sec

Player:

Snare @ 10.473 sec

Difference:

-27 ms
Result:
PERFECT
27 ms early
The exact timing thresholds should be configurable.
Initial suggested defaults:
±30 ms       PERFECT
±60 ms       GREAT
±100 ms      GOOD
±150 ms      OK
>150 ms      MISS
These values should eventually be configurable.
________________________________________
22. Early/Late Detection
Every matched note should store its timing error.
Use:
actual_time - expected_time
Therefore:
negative = early
positive = late
zero = exact
Example:
-27 ms → 27 ms early
+43 ms → 43 ms late
Display this feedback where appropriate.
________________________________________
23. Wrong Drum Detection
Do not only determine whether a note occurred.
Determine whether the correct drum was played.
Example:
Expected:
Snare @ 10.500
Player:
Kick @ 10.510
This should not count as a correct hit.
Classify it as:
WRONG DRUM
Potential result categories:
PERFECT
GREAT
GOOD
OK
EARLY
LATE
MISSED
WRONG DRUM
Avoid double-counting one physical MIDI event against multiple expected notes.
The matching algorithm should intelligently pair expected notes with player events.
________________________________________
24. Chords / Simultaneous Hits
The system must support simultaneous drum hits.
For example:
Kick + Hi-Hat
at exactly the same musical position.
The engine must recognize this as two expected events that can be independently matched.
Example:
Expected:
Kick   @ 2.000
Hi-Hat @ 2.000

Player:
Kick   @ 2.014
Hi-Hat @ 2.022
Both should be independently evaluated.
This is essential for normal drum grooves.
________________________________________
25. Accuracy Statistics
During and after practice, calculate:
Accuracy
Perfect hits
Great hits
Good hits
OK hits
Missed notes
Wrong drum hits
Early hits
Late hits
Average timing error
Maximum timing error
Current combo
Best combo
Example:
SONG COMPLETE

Accuracy:       94.7%

Perfect:        183
Great:           21
Good:             8
OK:               3
Missed:           8
Wrong:            3

Early:            9
Late:            12

Average error:   18 ms
Best combo:      37
________________________________________
26. Timing Graph
After a practice session, provide a timing visualization.
For example:
Timing Error

+100ms |          ●
 +50ms |     ● ●      ●
   0ms | ● ● ● ● ● ● ● ●
 -50ms |       ●   ●
-100ms |             ●
       └────────────────────────
                 Time →
This lets users see whether they consistently rush or drag.
The graph should eventually allow filtering by drum:
All
Kick
Snare
Hi-Hat
Toms
Ride
Crash
This could reveal things such as:
The player is consistently late on snare hits.
________________________________________
27. Main UI Layout
Design the application around the reference layout.
Suggested overall screen:
┌─────────────────────────────────────────────────────────────────────────────┐
│ Song Name                         BPM     4/4     1/8     Metronome          │
├──────────────┬──────────────────────────────────────────────────────────────┤
│              │                                                              │
│ CRASH        │                                                              │
│              │                                                              │
├──────────────┤                                                              │
│ RIDE         │                         NOTES                                │
│              │                                                              │
├──────────────┤                                                              │
│ HI-HAT       │             ●                                                │
│              │                   ●                                          │
├──────────────┤                                                              │
│ SNARE        │                       ●                                      │
│              │                                                              │
├──────────────┤                                                              │
│ KICK         │             ●                                                │
│              │                                                              │
├──────────────┤                                                              │
│ HI TOM       │                                                              │
├──────────────┤                                                              │
│ MID TOM      │                                                              │
├──────────────┤                                                              │
│ LOW TOM      │                                                              │
├──────────────┴──────────────────────────────────────────────────────────────┤
│                 1    &    2    &    3    &    4    &                       │
│                              ▲                                               │
├─────────────────────────────────────────────────────────────────────────────┤
│ ▶   00:42 / 03:21     LOOP      BPM: 95     SPEED: 80%      ACC: 94.7%      │
└─────────────────────────────────────────────────────────────────────────────┘
Keep the UI clean.
Avoid excessive buttons and visual clutter.
________________________________________
28. Visual Design
Use a dark theme.
The reference screenshot uses:
•	Very dark background
•	Subtle grid lines
•	Thin borders
•	Colored drum labels
•	Orange/yellow timing line
•	Minimal typography
•	Flat professional controls
Use this general aesthetic.
Suggested drum colors:
Crash    → coral/red
Ride     → orange
Hi-Hat   → yellow
Snare    → cyan/blue
Kick     → red
Hi Tom   → teal
Mid Tom  → blue
Low Tom  → purple
The exact colors can be adjusted for accessibility.
The UI should not look childish.
It should feel like professional music software.
________________________________________
29. Song Library
Eventually provide a library.
Example:
MY SONGS

Recently Played
Favorites
Practice

────────────────────────

Back In Black
Enter Sandman
Seven Nation Army
Smells Like Teen Spirit
My Groove
Allow users to import as many MIDI files as they want.
Songs should not be restricted to a fixed number.
________________________________________
30. Practice Progress
Eventually save practice results.
For every song, store:
Song
Date
Duration
BPM
Practice speed
Accuracy
Perfect %
Miss %
Average timing error
Best combo
Then allow the user to see improvement.
Example:
BACK IN BLACK

Session 1     72%
Session 2     78%
Session 3     84%
Session 4     89%
Session 5     94%
This should become a long-term learning tool rather than merely a playback program.
________________________________________
31. MIDI Device Handling
Handle:
•	Device connected
•	Device disconnected
•	Device reconnected
•	Device unavailable
•	Multiple devices
•	Invalid MIDI data
If the device disconnects during practice, display:
MIDI DEVICE DISCONNECTED

Please reconnect your drum kit.

[ Retry ]
Do not crash the application.
________________________________________
32. Latency Requirements
Latency is a major design requirement.
The application should minimize:
•	MIDI input latency
•	Audio output latency
•	GUI latency
•	Timing jitter
•	Unnecessary buffering
Do not use the GUI event loop as the timing source.
Do not calculate timing based on screen frames.
For example, do NOT do:
Every frame:
check if note was hit
Instead:
MIDI event timestamp
        ↓
High-resolution transport clock
        ↓
Accuracy engine
The UI should simply visualize the underlying timing data.
________________________________________
33. Threading
Separate real-time work from GUI work.
Suggested conceptual architecture:
REAL-TIME MIDI THREAD
        │
        ↓
MIDI EVENT QUEUE
        │
        ↓
PRACTICE ENGINE
        │
        ↓
RESULT QUEUE
        │
        ↓
GUI
Similarly:
AUDIO THREAD
      ↓
Metronome / backing audio
Avoid memory allocations and expensive operations inside time-critical audio/MIDI processing where possible.
________________________________________
34. Open Source
The project must be designed from the beginning as open-source software.
Provide:
README.md
LICENSE
CONTRIBUTING.md
CHANGELOG.md
Include clear setup instructions.
Document:
•	Building from source
•	MIDI setup
•	Supported platforms
•	Supported MIDI devices
•	Architecture
•	Contributing
•	Bug reporting
Prefer a permissive license such as MIT or Apache-2.0 unless there is a specific reason to choose another license.
________________________________________
35. Cross-Platform Goal
Primary target:
Windows
But architecture should allow:
Linux
macOS
Do not introduce unnecessary Windows-only dependencies into the core application.
________________________________________
36. Keyboard Controls
Eventually support useful keyboard shortcuts.
For example:
Space       Play/Pause
R           Restart
L           Toggle Loop
+/-         BPM
[/]         Playback speed
M           Toggle Metronome
C           Toggle Count-in
Document all shortcuts.
________________________________________
37. Important UX Principle
The application should always answer three questions for the drummer:
What do I play?
The drum lanes show this.
When do I play it?
The timeline, hit line, and metronome show this.
How well did I play it?
The accuracy engine and statistics show this.
The entire UI should reinforce these three concepts.
________________________________________
38. Initial MVP
Do not attempt to implement every feature simultaneously.
Build in this order.
Phase 1 — MIDI Foundation
Implement:
•	Rust project
•	MIDI device detection
•	MIDI input
•	Live MIDI event monitor
•	Device selection
•	Setup wizard
•	MIDI drum mapping
•	Save/load mapping
At the end of Phase 1:
NUX DM-1 → USB → Application

Hit snare

Application:
SNARE
MIDI 38
Velocity 103
must work reliably.
________________________________________
Phase 2 — MIDI File Engine
Implement:
•	.mid import
•	MIDI parsing
•	Drum track selection
•	Tempo extraction
•	Time signature extraction
•	Expected note representation
•	Timeline transport
At the end:
A MIDI file should display correctly on the drum lanes.
________________________________________
Phase 3 — Metronome
Implement:
•	BPM
•	Time signature
•	Subdivision
•	Audible clicks
•	Visual beat counter
•	1 & 2 & 3 & 4 &
•	Accents
•	Count-in
This is a major milestone.
________________________________________
Phase 4 — Practice Engine
Implement:
•	Player MIDI input
•	Expected note matching
•	Timing difference
•	Early/late detection
•	Miss detection
•	Wrong drum detection
•	Simultaneous notes
•	Accuracy percentage
At the end:
A user should be able to play a MIDI song and receive a meaningful score.
________________________________________
Phase 5 — Practice Features
Implement:
•	Looping
•	Playback speed
•	Restart
•	BPM adjustment
•	Practice sections
•	Timing graph
•	Detailed results
________________________________________
Phase 6 — Audio
Implement:
•	Backing tracks
•	Volume controls
•	Metronome mixing
•	Audio synchronization
•	Time stretching where practical
________________________________________
Phase 7 — Library and Progress
Implement:
•	Song library
•	Favorites
•	Practice history
•	Accuracy history
•	Progress graphs
•	Saved configurations
________________________________________
39. Example Complete User Workflow
A new user launches the application.
Step 1
WELCOME

Connect your MIDI drum kit.

[ Select MIDI Device ]
Step 2
NUX DM-1 detected.

Let's configure your kit.

Hit your CRASH.
User hits crash.
✓ Crash = MIDI 49
Continue through:
Ride
Hi-Hat
Snare
Kick
Hi Tom
Mid Tom
Low Tom
Step 3
User imports:
song.mid
Application detects:
Tempo: 120 BPM
Time Signature: 4/4
Drum Track: Track 10
Step 4
User chooses:
Practice Speed: 60%
Subdivision: 1/8
Metronome: ON
Count-in: 1 bar
Step 5
Application displays:
4

3

2

1

GO
Step 6
The song begins.
The top displays:
1   &   2   &   3   &   4   &
▲
while notes move toward the hit line.
Step 7
User plays their physical drum kit.
The MIDI engine receives:
Kick
Hi-Hat
Snare
Hi-Hat
...
The accuracy engine compares those events against the expected MIDI.
Step 8
User completes the loop.
Results:
ACCURACY: 87.4%

Perfect:       122
Great:          31
Good:           12
OK:              4
Missed:         14
Wrong:           3

Early:          18
Late:            9

Average Error:  32 ms
Best Combo:     24
The user then loops the difficult section and tries again.
________________________________________
40. Future Features
Design the architecture so these can be added later without rewriting the core.
Potential future features:
•	Automatic difficulty adjustment
•	Practice recommendations
•	Rudiment exercises
•	Built-in drum lessons
•	Stick-control exercises
•	Timing exercises
•	Custom practice patterns
•	MIDI recording
•	Record and replay user performance
•	Compare two performances
•	Export performance as MIDI
•	Import/export drum mappings
•	Multiple drum kits
•	Cloud sync if desired
•	User profiles
•	Achievements
•	Progress tracking
•	Song-specific practice statistics
•	Individual drum accuracy
•	Velocity analysis
•	Ghost-note detection
•	Dynamic analysis
•	Tempo stability analysis
•	Polyrhythm support
•	Odd meters
•	Custom subdivisions
•	Custom exercises
Do not implement these until the core MIDI → transport → metronome → practice → scoring pipeline is reliable.
________________________________________
41. Development Philosophy
Prioritize correctness over visual polish.
The most important parts of this application are:
1.	Reliable MIDI input
2.	Accurate timestamps
3.	Reliable transport clock
4.	Accurate metronome
5.	Correct MIDI parsing
6.	Correct note matching
7.	Accurate scoring
8.	Low latency
The GUI can be improved later.
Do not sacrifice timing accuracy for visual effects.
________________________________________
42. Testing
Create automated tests for the accuracy engine.
For example:
Expected:
Snare @ 1000 ms

Actual:
Snare @ 1000 ms

Expected:
PERFECT
Expected:
Snare @ 1000 ms

Actual:
Snare @ 1040 ms

Expected:
GREAT
Expected:
Snare @ 1000 ms

Actual:
Kick @ 1005 ms

Expected:
WRONG DRUM
Expected:
Kick @ 1000 ms
Snare @ 1000 ms

Actual:
Kick @ 1010 ms
Snare @ 1020 ms

Expected:
Both correctly matched
Also test:
•	Multiple simultaneous hits
•	Repeated identical notes
•	Very fast drum patterns
•	Notes close together in time
•	MIDI device disconnection
•	Loop boundaries
•	Tempo changes
•	Time signature changes
•	Playback speed changes
________________________________________
43. Performance Requirement
The application should remain responsive even with:
•	Large MIDI files
•	Dense drum patterns
•	Long songs
•	Continuous MIDI input
•	Audio playback
•	Visual timeline animation
Do not perform expensive MIDI parsing or analysis on the GUI thread.
Preprocess MIDI files when they are loaded.
________________________________________
44. Final Product Identity
This application should NOT be marketed internally as merely:
"A MIDI rhythm game."
It should be thought of as:
An open-source, low-latency MIDI drum practice workstation.
Its primary purpose is to help a real drummer improve.
The software should combine:
MIDI drum kit
      +
MIDI songs
      +
Visual drum timeline
      +
Metronome
      +
Musical counting
      +
Loop practice
      +
Variable tempo
      +
Backing tracks
      +
Precise timing analysis
      +
Performance statistics
into one application.
The final experience should feel like a dedicated piece of music-practice software rather than a generic desktop application.
________________________________________
45. First Development Goal
Do NOT start by building the complete application.
Start with the smallest technically meaningful prototype:
USB MIDI DRUM KIT
        ↓
MIDI INPUT
        ↓
DISPLAY MIDI NOTE + VELOCITY + TIMESTAMP
        ↓
SETUP WIZARD
        ↓
MAP:
Crash
Ride
Hi-Hat
Snare
Kick
Hi Tom
Mid Tom
Low Tom
        ↓
LOAD MIDI FILE
        ↓
DISPLAY DRUM NOTES
        ↓
MASTER TRANSPORT CLOCK
        ↓
METRONOME
        ↓
COMPARE EXPECTED VS ACTUAL
        ↓
ACCURACY SCORE
Only after this pipeline works reliably should the polished UI, backing tracks, song library, progress tracking, and advanced features be added.
The final goal is a fully open-source, low-latency, cross-platform drum-learning application where any MIDI-compatible electronic drum kit can be connected and used to practice real drum MIDI files with synchronized musical counting and precise performance feedback.

