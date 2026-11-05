//! Reject unrelated speech before asking the LED-command model for an action.

pub fn is_control_request(text: &str) -> bool {
    let text = text.to_lowercase();
    let words: Vec<&str> = text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty()).collect();
    let has_target = words.iter().any(|word| {
        matches!(*word, "led" | "lead" | "gpio" | "pin" | "diodo" | "luce" | "luci" | "lampada" | "lampadina")
            || ["gpio", "pin"].iter().any(|prefix| word.strip_prefix(prefix)
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())))
    });
    let has_action = words.iter().any(|word| {
        *word == "stop" || ["accend", "spegn", "attiv", "disattiv", "lampegg", "ferm", "interromp", "arrest"]
            .iter().any(|stem| word.starts_with(stem))
    });
    // The small STT model can turn "LED sul pin" into "letto super in".
    // Accept an explicit blink request with one unambiguous pin and interval.
    let numeric_blink = words.iter().any(|word| word.starts_with("lampegg"))
        && crate::voice_numbers::extract(&text).is_ok_and(|numbers| numbers.pin.is_some() && numbers.interval_s.is_some());
    (has_target || numeric_blink) && has_action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_speech_cannot_be_turned_into_a_gpio_action() {
        for text in ["oggi è una bella giornata", "questo non funziona", "ascolta la mia storia", "ferma il programma", "il led non funziona"] {
            assert!(!is_control_request(text), "{text}");
        }
    }

    #[test]
    fn normal_led_commands_and_transcribed_target_variants_are_kept() {
        for text in [
            "led diciassette accendi", "spegni lead sul pin diciassette",
            "fai lampeggiare led sul pin diciassette intervallo 0.25 secondi",
            "ferma il lampeggio sul pin17", "puoi spegnere il GPIO 17", "stop led pin 17",
            "fai lampeggiare letto super in 17 intervallo 0.25 secondi",
        ] {
            assert!(is_control_request(text), "{text}");
        }
    }
}
