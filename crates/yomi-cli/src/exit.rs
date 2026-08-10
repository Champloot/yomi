//! Коды возврата. Утилита должна быть пригодна для скриптов,
//! поэтому коды осмысленные, а не «1 на всё».

use std::process::ExitCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Code {
    Ok = 0,
    /// Общая ошибка выполнения.
    Failure = 1,
    /// Неверное использование: плохие аргументы, битый конфиг.
    Usage = 2,
    /// Запрошенное не найдено.
    NotFound = 4,
    /// Сеть недоступна или источник не отвечает.
    Network = 5,
    /// Возможность ещё не реализована.
    NotImplemented = 6,
    /// Внутренняя ошибка — так быть не должно.
    Internal = 70,
}

impl From<Code> for ExitCode {
    fn from(c: Code) -> Self {
        ExitCode::from(c as u8)
    }
}

impl Code {
    /// Сопоставляет ошибку с кодом возврата.
    ///
    /// Проверяются все типы ошибок, которые могут всплыть наружу: у
    /// каждого крейта своё перечисление, и забыть один из них означает
    /// молча вернуть общий код 1 вместо осмысленного.
    pub fn from_error(err: &anyhow::Error) -> Self {
        use yomi_core::Error as Core;
        match err.downcast_ref::<Core>() {
            Some(Core::NotFound(_)) | Some(Core::SourceNotFound(_)) => return Code::NotFound,
            Some(Core::ConfigParse { .. }) => return Code::Usage,
            Some(Core::NotImplemented(_)) => return Code::NotImplemented,
            Some(Core::Network(_)) => return Code::Network,
            Some(Core::BadResponse(_)) => return Code::Failure,
            _ => {}
        }

        use yomi_db::Error as Db;
        match err.downcast_ref::<Db>() {
            Some(Db::NotFound(_)) => return Code::NotFound,
            Some(Db::SchemaTooNew { .. }) => return Code::Usage,
            _ => {}
        }

        use yomi_viewer::Error as Viewer;
        match err.downcast_ref::<Viewer>() {
            Some(Viewer::UnsupportedFormat(_)) | Some(Viewer::NoPages(_)) => Code::Usage,
            Some(Viewer::PageOutOfRange(_)) => Code::NotFound,
            _ => Code::Failure,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_maps_to_four() {
        let err = anyhow::Error::new(yomi_core::Error::NotFound("x".into()));
        assert_eq!(Code::from_error(&err), Code::NotFound);
    }

    #[test]
    fn network_failure_maps_to_five() {
        // Скрипт должен отличать «сервис недоступен» от «нет такого
        // тайтла»: в первом случае имеет смысл повторить позже.
        let err = anyhow::Error::new(yomi_core::Error::Network("таймаут".into()));
        assert_eq!(Code::from_error(&err), Code::Network);
    }

    #[test]
    fn database_not_found_maps_to_four() {
        let err = anyhow::Error::new(yomi_db::Error::NotFound("тайтл 999".into()));
        assert_eq!(Code::from_error(&err), Code::NotFound);
    }

    #[test]
    fn unsupported_format_is_a_usage_error() {
        let err = anyhow::Error::new(yomi_viewer::Error::UnsupportedFormat("/x.txt".into()));
        assert_eq!(Code::from_error(&err), Code::Usage);
    }

    #[test]
    fn unknown_error_maps_to_generic_failure() {
        let err = anyhow::anyhow!("что-то пошло не так");
        assert_eq!(Code::from_error(&err), Code::Failure);
    }
}
