#[derive(Debug, PartialEq)]
pub enum BiometricStatus {
    Available,
    NotConfiguredForUser,
    DisabledByPolicy,
    NotAvailable,
}

impl BiometricStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            BiometricStatus::Available => "available",
            BiometricStatus::NotConfiguredForUser => "not_configured",
            BiometricStatus::DisabledByPolicy => "disabled_by_policy",
            BiometricStatus::NotAvailable => "not_available",
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::BiometricStatus;
    use windows::Security::Credentials::UI::{
        UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
    };
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };
    use zeroize::Zeroizing;

    pub async fn check_availability() -> BiometricStatus {
        let result = tokio::task::spawn_blocking(|| {
            UserConsentVerifier::CheckAvailabilityAsync()
                .and_then(|op| op.get())
        })
        .await;

        let availability = match result {
            Ok(Ok(a)) => a,
            _ => return BiometricStatus::NotAvailable,
        };

        match availability {
            UserConsentVerifierAvailability::Available => BiometricStatus::Available,
            // DeviceBusy means the sensor is temporarily busy; treat as available.
            UserConsentVerifierAvailability::DeviceBusy => BiometricStatus::Available,
            UserConsentVerifierAvailability::DisabledByPolicy => BiometricStatus::DisabledByPolicy,
            UserConsentVerifierAvailability::NotConfiguredForUser => {
                BiometricStatus::NotConfiguredForUser
            }
            _ => BiometricStatus::NotAvailable,
        }
    }

    pub async fn request_verification(message: &str) -> Result<bool, String> {
        let msg = windows::core::HSTRING::from(message);
        let result = tokio::task::spawn_blocking(move || {
            UserConsentVerifier::RequestVerificationAsync(&msg)
                .and_then(|op| op.get())
        })
        .await
        .map_err(|e| format!("biometric task failed: {e}"))?
        .map_err(|e| format!("biometric request failed: {e}"))?;

        match result {
            UserConsentVerificationResult::Verified => Ok(true),
            // DeviceBusy or Canceled → not a hard error; caller can retry.
            UserConsentVerificationResult::DeviceBusy
            | UserConsentVerificationResult::Canceled => Ok(false),
            other => Err(format!("biometric verification failed: code {}", other.0)),
        }
    }

    pub fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>, String> {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };

        unsafe {
            CryptProtectData(
                &mut input,
                windows::core::w!("cryptenv_biometric"),
                None,
                None,
                None,
                0,
                &mut output,
            )
            .map_err(|e| format!("DPAPI protect failed: {e}"))?;
        }

        let blob =
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();

        // LocalFree frees the OS-allocated buffer — do NOT use the Rust allocator here.
        unsafe { LocalFree(HLOCAL(output.pbData.cast())) };

        Ok(blob)
    }

    pub fn dpapi_unprotect(blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: blob.len() as u32,
            pbData: blob.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };

        unsafe {
            CryptUnprotectData(
                &mut input,
                None,
                None,
                None,
                None,
                0,
                &mut output,
            )
            .map_err(|e| format!("DPAPI unprotect failed: {e}"))?;
        }

        let plaintext =
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();

        // Zeroize the OS buffer before freeing so the plaintext doesn't linger in memory.
        unsafe {
            std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
            LocalFree(HLOCAL(output.pbData.cast()));
        }

        Ok(Zeroizing::new(plaintext))
    }
}

#[cfg(target_os = "windows")]
pub use windows_impl::{check_availability, dpapi_protect, dpapi_unprotect, request_verification};

#[cfg(not(target_os = "windows"))]
pub async fn check_availability() -> BiometricStatus {
    BiometricStatus::NotAvailable
}

#[cfg(not(target_os = "windows"))]
pub async fn request_verification(_message: &str) -> Result<bool, String> {
    Ok(false)
}
