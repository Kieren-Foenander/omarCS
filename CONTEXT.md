# omarCS Match Analysis

omarCS turns a local Counter-Strike 2 demo into a personal match report for the Omarchy shell.

## Language

**Demo**:
A Counter-Strike 2 replay file containing the event and entity history from which omarCS derives a match report.
_Avoid_: Replay, recording

**Match Facts**:
The normalized, player-resolved rounds, events, shots, damage, and tick observations extracted from one demo. Match Facts contain no coaching or presentation decisions.
_Avoid_: Parsed data, dataframe, model

**Match Report**:
The persisted result for one player in one demo, including statistics, mechanics, sprays, and coaching insights.
_Avoid_: Analysis payload, result object

**Engagement**:
A continuous interval beginning when a living enemy first becomes visible and ending with damage or expiry of the one-second window.
_Avoid_: Encounter, duel window

**Spray**:
A qualifying burst of at least five bullets from a supported rifle, beginning while settled and associated with a visible enemy.
_Avoid_: Burst, recoil sequence

**Dashboard Summary**:
The atomic JSON projection watched by the QML dashboard, containing recent Match Reports, trends, status, and aggregated Spray data.
_Avoid_: summary.json payload, UI state
