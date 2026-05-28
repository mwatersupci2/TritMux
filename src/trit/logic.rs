use crate::trit::core::Trit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriBool {
    False,
    Unknown,
    True,
}

impl TriBool {
    pub fn not(self) -> Self {
        match self {
            TriBool::False => TriBool::True,
            TriBool::Unknown => TriBool::Unknown,
            TriBool::True => TriBool::False,
        }
    }

    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (TriBool::False, _) | (_, TriBool::False) => TriBool::False,
            (TriBool::Unknown, _) | (_, TriBool::Unknown) => TriBool::Unknown,
            (TriBool::True, TriBool::True) => TriBool::True,
        }
    }

    pub fn or(self, other: Self) -> Self {
        match (self, other) {
            (TriBool::True, _) | (_, TriBool::True) => TriBool::True,
            (TriBool::Unknown, _) | (_, TriBool::Unknown) => TriBool::Unknown,
            (TriBool::False, TriBool::False) => TriBool::False,
        }
    }

    pub fn from_trit(trit: Trit) -> Self {
        match trit {
            Trit::Negative => TriBool::False,
            Trit::Neutral => TriBool::Unknown,
            Trit::Positive => TriBool::True,
        }
    }

    pub fn to_trit(self) -> Trit {
        match self {
            TriBool::False => Trit::Negative,
            TriBool::Unknown => Trit::Neutral,
            TriBool::True => Trit::Positive,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kleene_logic_tables_hold() {
        assert_eq!(TriBool::False.and(TriBool::Unknown), TriBool::False);
        assert_eq!(TriBool::True.and(TriBool::Unknown), TriBool::Unknown);
        assert_eq!(TriBool::False.or(TriBool::Unknown), TriBool::Unknown);
        assert_eq!(TriBool::True.or(TriBool::Unknown), TriBool::True);
        assert_eq!(TriBool::Unknown.not(), TriBool::Unknown);
    }
}
