use citrine_gb_pkm::Language;
const JAPANESE_BOYS: &[&str] = &[
    "コウキ",
    "サトシ",
    "たけし",
    "ゆうき",
    "レッド",
    "ゴールド",
    "ひろき",
    "まさと",
    "ゆうた",
    "しょうた",
    "だいき",
    "けんた",
    "りょう",
    "はると",
    "そうた",
    "ユウキ",
    "ケンジ",
    "アキラ",
    "ヒロシ",
    "マサキ",
    "ナオキ",
    "カズマ",
    "タクミ",
    "リク",
    "ゆうと",
    "れん",
    "こうた",
    "しゅん",
    "たくや",
    "サトシ２",
    "ヒビキ",
    "シルバー",
    "ケン",
    "まなと",
    "かいと",
];

const JAPANESE_GIRLS: &[&str] = &[
    "ヒカリ",
    "あやか",
    "さくら",
    "みさき",
    "ゆい",
    "あおい",
    "ひなた",
    "めぐみ",
    "まい",
    "ちひろ",
    "つばさ",
    "コトネ",
    "ユウ",
    "りん",
    "かすみ",
    "ハルカ",
    "みく",
    "なつみ",
    "えりか",
    "ゆか",
];

const ENGLISH_BOYS: &[&str] = &[
    "Lucas", "Ethan", "ASH", "RED", "GOLD", "Jake", "Ryan", "Chris", "Mike", "Tyler", "Josh",
    "Matt", "JOHN", "Alex7", "Nick", "Dan", "Brandon", "Kevin", "Jason", "Zach", "Kyle", "Austin",
    "Brian", "Eric", "Sam", "Ben", "Tom", "Jack", "Liam", "Ollie", "DAVID", "Steven", "Jordan",
    "Cody", "Dylan", "Logan", "Luke", "Jacob", "Kai", "Max", "Joe1", "Mike99", "Danny",
];

const ENGLISH_GIRLS: &[&str] = &[
    "Dawn", "Lyra", "Emma", "Kayla", "Sarah", "Ashley", "Megan", "Chloe", "Amy", "Katie", "Lily",
    "Hannah", "Riley", "Jess", "MAY", "Misty", "Ellie", "Abby", "Becca", "Jenny",
];

const FRENCH_BOYS: &[&str] = &[
    "Lucas", "Pierre", "Julien", "Hugo", "Théo", "Maxime", "Kévin", "Thomas", "Nicolas", "Antoine",
    "Clément", "Romain", "Quentin", "Bastien", "Florian", "Jérémy", "Sacha", "LOUIS", "Enzo",
    "Nathan", "Yanis", "Rémi", "Benoît", "Max59", "Alex", "Jules",
];

const FRENCH_GIRLS: &[&str] = &[
    "Aurore", "Camille", "Manon", "Léa", "Océane", "Chloé", "Emma", "Inès", "Pauline", "Laura",
    "Marie", "Lucie", "Clara", "Julie", "Anaïs", "Margot", "Sarah", "Lola", "Zoé", "Elsa",
];

const GERMAN_BOYS: &[&str] = &[
    "Lukas", "Jonas", "Leon", "Felix", "Tim", "Jan", "Niklas", "Paul", "Finn", "Luca", "Max",
    "Moritz", "Tobias", "Florian", "Kevin", "Dennis", "Marcel", "Jannik", "Jasper", "Nils", "Ben",
    "Fabian", "Sven", "Timo", "Daniel", "Basti", "Flo", "Emil", "Henrik", "MAX", "Tim94", "Kai",
];

const GERMAN_GIRLS: &[&str] = &[
    "Lucia", "Lena", "Anna", "Sophie", "Laura", "Julia", "Lisa", "Marie", "Lea", "Mia", "Hannah",
    "Jana", "Nele", "Katha", "Leonie", "Sarah", "Lara", "Emily", "Paula", "Jule",
];

const SPANISH_BOYS: &[&str] = &[
    "Lucas", "Pablo", "Javi", "Carlos", "Dani", "Sergio", "Álvaro", "Alex", "Adrián", "Diego",
    "Iván", "Rubén", "Raúl", "Jorge", "Mario", "Hugo", "Andrés", "Miguel", "Óscar", "David",
    "Juanma", "Nacho", "Pepe", "Isma", "Santi", "Fran", "DANI", "Ash", "Edu",
];

const SPANISH_GIRLS: &[&str] = &[
    "Maya", "Lucía", "Marta", "Laura", "Paula", "Sara", "Elena", "Nerea", "Irene", "Carmen",
    "Alba", "Noelia", "Cris", "Andrea", "Ainhoa", "Clara", "Rocío", "Julia", "Bea", "Lidia",
];

const ITALIAN_BOYS: &[&str] = &[
    "Lucas", "Marco", "Luca", "Andrea", "Matteo", "Davide", "Fede", "Lorenzo", "Simone", "Alessio",
    "Ricky", "Tommaso", "Pietro", "Nicolò", "Stefano", "Paolo", "Gianni", "Ale", "Teo", "MARCO",
    "Luca98", "Ash", "Gabri", "Edo", "Samu", "Dario",
];

const ITALIAN_GIRLS: &[&str] = &[
    "Lucinda", "Giulia", "Sara", "Chiara", "Martina", "Elisa", "Alice", "Giorgia", "Francy",
    "Vale", "Sofia", "Aurora", "Ilaria", "Silvia", "Marta", "Gaia", "Anna", "Noemi", "Elena",
    "Bea",
];

const KOREAN_BOYS: &[&str] = &[
    "민수",
    "지우",
    "도윤",
    "지훈",
    "현우",
    "준서",
    "예준",
    "시우",
    "하준",
    "지호",
    "서준",
    "은우",
    "태양",
    "레드",
    "골드",
    "민호",
    "성민",
    "동현",
    "재민",
    "승우",
    "영수",
    "철수",
    "광",
    "다이아",
];

const KOREAN_GIRLS: &[&str] = &[
    "하늘", "서연", "유진", "수아", "민지", "유나", "채원", "지민", "한별", "펄", "지아", "서윤",
    "하은", "예린", "수빈", "다은", "소연", "윤아", "나래", "보라",
];

pub fn names(language: Language, female: bool) -> &'static [&'static str] {
    match (language, female) {
        (Language::Japanese, false) => JAPANESE_BOYS,
        (Language::Japanese, true) => JAPANESE_GIRLS,
        (Language::English, false) => ENGLISH_BOYS,
        (Language::English, true) => ENGLISH_GIRLS,
        (Language::French, false) => FRENCH_BOYS,
        (Language::French, true) => FRENCH_GIRLS,
        (Language::German, false) => GERMAN_BOYS,
        (Language::German, true) => GERMAN_GIRLS,
        (Language::Spanish, false) => SPANISH_BOYS,
        (Language::Spanish, true) => SPANISH_GIRLS,
        (Language::Italian, false) => ITALIAN_BOYS,
        (Language::Italian, true) => ITALIAN_GIRLS,
        (Language::Korean, false) => KOREAN_BOYS,
        (Language::Korean, true) => KOREAN_GIRLS,
        (Language::Other(_), _) => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_pkm::legality::name_limits;
    use citrine_gb_pkm::{Gen, Pokemon};

    #[test]
    fn every_name_fits_its_language_and_the_game_can_write_it() {
        for language in [
            Language::Japanese,
            Language::English,
            Language::French,
            Language::German,
            Language::Spanish,
            Language::Italian,
            Language::Korean,
        ] {
            let (_, most) = name_limits(language);
            let mut pkm = Pokemon::new(Gen::Four);
            pkm.language = language;
            for female in [false, true] {
                let list = names(language, female);
                assert!(list.len() >= 20, "{language:?}");
                for name in list {
                    assert!(name.chars().count() <= most, "{name}");
                    assert!(pkm.set_ot(name).is_ok(), "{name}");
                }
            }
        }
    }
}
