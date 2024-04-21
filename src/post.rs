use std::{fmt::Display, str::FromStr};

#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
pub struct DailyPostDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Display for DailyPostDate {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl FromStr for DailyPostDate {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(3, '-');

        let year = parts
            .next()
            .unwrap()
            .parse::<u16>()
            .map_err(|_| "Invalid year")?;
        let month = parts
            .next()
            .ok_or("Missing month")?
            .parse::<u8>()
            .map_err(|_| "Invalid month")?;
        let day = parts
            .next()
            .ok_or("Missing day")?
            .parse::<u8>()
            .map_err(|_| "Invalid day")?;

        Ok(DailyPostDate { year, month, day })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyPostTitle {
    pub title: String,
    pub date: DailyPostDate,
    pub href: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyPostCategory {
    pub posts: Vec<DailyPostTitle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyPost {
    pub content_html: String,
}
