use crate::error::Error;

/// Pagination defaults from the public contract.
pub const DEFAULT_PAGE: u32 = 1;
pub const DEFAULT_PAGE_SIZE: u32 = 20;
pub const MAX_PAGE_SIZE: u32 = 100;

/// Pagination is opt-in: absent is the default, rejected when malformed,
/// non-positive or overflowing.
pub fn parse_page_number(param: Option<&str>) -> Result<u32, Error> {
    match param {
        None => Ok(DEFAULT_PAGE),
        Some(value) => value
            .parse::<u32>()
            .map_err(|_| Error::InvalidInput)
            .and_then(|page| {
                if page >= 1 {
                    Ok(page)
                } else {
                    Err(Error::InvalidInput)
                }
            }),
    }
}

pub fn parse_page_size(param: Option<&str>) -> Result<u32, Error> {
    match param {
        None => Ok(DEFAULT_PAGE_SIZE),
        Some(value) => value
            .parse::<u32>()
            .map_err(|_| Error::InvalidInput)
            .and_then(|size| {
                if size >= 1 {
                    Ok(size.min(MAX_PAGE_SIZE))
                } else {
                    Err(Error::InvalidInput)
                }
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_defaults_and_capping() {
        assert_eq!(parse_page_number(None).unwrap(), 1);
        assert!(matches!(
            parse_page_number(Some("abc")),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            parse_page_number(Some("0")),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            parse_page_number(Some("-2")),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            parse_page_number(Some("99999999999999999999999")),
            Err(Error::InvalidInput)
        ));
        assert_eq!(parse_page_number(Some("3")).unwrap(), 3);

        assert_eq!(parse_page_size(None).unwrap(), 20);
        assert!(matches!(
            parse_page_size(Some("abc")),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            parse_page_size(Some("0")),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            parse_page_size(Some("-1")),
            Err(Error::InvalidInput)
        ));
        assert_eq!(parse_page_size(Some("500")).unwrap(), MAX_PAGE_SIZE);
        assert_eq!(parse_page_size(Some("7")).unwrap(), 7);
    }
}
