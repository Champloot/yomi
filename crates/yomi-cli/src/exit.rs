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
    /// Сопоставляет ошибку ядра с кодом возврата.
    pub fn from_error(err: &anyhow::Error) -> Self {
        use yomi_core::Error as E;
        match err.downcast_ref::<E>() {
            Some(E::NotFound(_)) | Some(E::SourceNotFound(_)) => Code::NotFound,
            Some(E::ConfigParse { .. }) => Code::Usage,
            Some(E::NotImplemented(_)) => Code::NotImplemented,
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
    fn unknown_error_maps_to_generic_failure() {
        let err = anyhow::anyhow!("что-то пошло не так");
        assert_eq!(Code::from_error(&err), Code::Failure);
    }
}
