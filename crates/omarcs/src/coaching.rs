use crate::mechanics::MechanicsMetrics;
use crate::metrics::PlayerMetrics;

const DEFAULT_NOTE: &str = "No obvious outlier this match; compare it with your next few games.";

pub fn calculate(stats: &PlayerMetrics, mechanics: &MechanicsMetrics) -> Vec<String> {
    let mut insights = Vec::new();

    if mechanics.mechanics_engagements >= 3 && python_or(mechanics.crosshair_placement, 0.0) > 10.0
    {
        insights.push(format!(
            "Crosshair correction averaged {:.1}°; pre-aim closer to likely head positions.",
            mechanics.crosshair_placement.unwrap_or(0.0)
        ));
    }
    if mechanics.mechanics_engagements >= 3 && python_or(mechanics.time_to_damage_ms, 0.0) > 650.0 {
        insights.push(format!(
            "Time to damage was {:.0} ms; review whether placement or first-shot accuracy delayed fights.",
            mechanics.time_to_damage_ms.unwrap_or(0.0)
        ));
    }
    if mechanics.counter_strafe_shots >= 5
        && python_or(mechanics.counter_strafe_percent, 100.0) < 70.0
    {
        insights.push(format!(
            "Only {:.0}% of rifle shots were fully settled; finish the counter-strafe before firing.",
            mechanics.counter_strafe_percent.unwrap_or(100.0)
        ));
    }
    if stats.opening_deaths > stats.opening_kills {
        insights.push(
            "Opening duels cost more rounds than they created; review your first-contact fights."
                .to_owned(),
        );
    }
    if stats.friends_flashed > 1 {
        insights.push(format!(
            "You flashed teammates {} times; tighten flash timing and calls.",
            stats.friends_flashed
        ));
    }
    if stats.utility_damage < 10.max(stats.rounds as i32 * 2) {
        insights
            .push("Utility damage was quiet; look for earlier HE and molotov value.".to_owned());
    }
    if stats.traded_deaths < 1.max(stats.deaths / 3) && stats.deaths >= 6 {
        insights.push(
            "Few deaths were traded; check spacing and whether teammates could follow your fights."
                .to_owned(),
        );
    }
    if stats.adr >= 90.0 {
        insights.push("High-impact damage game—your ADR was above 90.".to_owned());
    }

    insights.truncate(3);
    if insights.is_empty() {
        insights.push(DEFAULT_NOTE.to_owned());
    }
    insights
}

fn python_or(value: Option<f64>, default: f64) -> f64 {
    match value {
        Some(value) if value != 0.0 => value,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_facts::PlayerId;
    use crate::mechanics;

    fn quiet_stats() -> PlayerMetrics {
        PlayerMetrics {
            steam_id: PlayerId(76_561_198_000_000_001),
            name: "Kieren".to_owned(),
            kills: 0,
            deaths: 0,
            assists: 0,
            kd: 0.0,
            adr: 50.0,
            kast: 50.0,
            rating: 1.0,
            headshot_percent: 0.0,
            opening_kills: 0,
            opening_deaths: 0,
            trade_kills: 0,
            traded_deaths: 0,
            utility_damage: 20,
            enemies_flashed: 0,
            friends_flashed: 0,
            enemy_flash_seconds: 0.0,
            rounds: 5,
            rounds_for: 3,
            rounds_against: 2,
            result: "W",
        }
    }

    #[test]
    fn returns_default_note_when_no_rule_fires() {
        let notes = calculate(&quiet_stats(), &mechanics::empty());
        assert_eq!(notes, vec![DEFAULT_NOTE.to_owned()]);
    }

    #[test]
    fn matches_python_coaching_fixture() {
        let mut stats = quiet_stats();
        let mut mechanics = mechanics::empty();

        mechanics.mechanics_engagements = 3;
        mechanics.crosshair_placement = Some(12.34);
        assert_eq!(
            calculate(&stats, &mechanics),
            vec![
                "Crosshair correction averaged 12.3°; pre-aim closer to likely head positions."
                    .to_owned()
            ]
        );

        mechanics = mechanics::empty();
        mechanics.mechanics_engagements = 3;
        mechanics.time_to_damage_ms = Some(651.4);
        assert_eq!(
            calculate(&stats, &mechanics),
            vec![
                "Time to damage was 651 ms; review whether placement or first-shot accuracy delayed fights."
                    .to_owned()
            ]
        );

        mechanics = mechanics::empty();
        mechanics.counter_strafe_shots = 5;
        mechanics.counter_strafe_percent = Some(69.4);
        assert_eq!(
            calculate(&stats, &mechanics),
            vec![
                "Only 69% of rifle shots were fully settled; finish the counter-strafe before firing."
                    .to_owned()
            ]
        );

        mechanics = mechanics::empty();
        mechanics.counter_strafe_shots = 5;
        mechanics.counter_strafe_percent = Some(0.0);
        assert_eq!(calculate(&stats, &mechanics), vec![DEFAULT_NOTE.to_owned()]);

        stats = quiet_stats();
        stats.opening_deaths = 2;
        stats.opening_kills = 1;
        assert_eq!(
            calculate(&stats, &mechanics::empty()),
            vec![
                "Opening duels cost more rounds than they created; review your first-contact fights."
                    .to_owned()
            ]
        );

        stats = quiet_stats();
        stats.friends_flashed = 2;
        assert_eq!(
            calculate(&stats, &mechanics::empty()),
            vec!["You flashed teammates 2 times; tighten flash timing and calls.".to_owned()]
        );

        stats = quiet_stats();
        stats.utility_damage = 9;
        assert_eq!(
            calculate(&stats, &mechanics::empty()),
            vec!["Utility damage was quiet; look for earlier HE and molotov value.".to_owned()]
        );

        stats = quiet_stats();
        stats.deaths = 6;
        stats.traded_deaths = 1;
        assert_eq!(
            calculate(&stats, &mechanics::empty()),
            vec![
                "Few deaths were traded; check spacing and whether teammates could follow your fights."
                    .to_owned()
            ]
        );

        stats = quiet_stats();
        stats.adr = 90.0;
        assert_eq!(
            calculate(&stats, &mechanics::empty()),
            vec!["High-impact damage game—your ADR was above 90.".to_owned()]
        );
    }

    #[test]
    fn keeps_python_priority_and_caps_at_three() {
        let mut stats = quiet_stats();
        stats.opening_deaths = 2;
        let mut mechanics = mechanics::empty();
        mechanics.mechanics_engagements = 3;
        mechanics.crosshair_placement = Some(11.0);
        mechanics.time_to_damage_ms = Some(700.0);
        mechanics.counter_strafe_shots = 5;
        mechanics.counter_strafe_percent = Some(50.0);

        let notes = calculate(&stats, &mechanics);
        assert_eq!(
            notes,
            vec![
                "Crosshair correction averaged 11.0°; pre-aim closer to likely head positions."
                    .to_owned(),
                "Time to damage was 700 ms; review whether placement or first-shot accuracy delayed fights."
                    .to_owned(),
                "Only 50% of rifle shots were fully settled; finish the counter-strafe before firing."
                    .to_owned(),
            ]
        );
        assert!(notes.len() <= 3);
    }
}
