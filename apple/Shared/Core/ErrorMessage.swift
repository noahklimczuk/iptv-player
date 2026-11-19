import Foundation

extension Error {
    /// What to tell the viewer about this error.
    ///
    /// The core's errors carry a sentence already worded for the viewer, but UniFFI's
    /// generated `errorDescription` prints the enum case around it
    /// (`AuroraError.Network(message: "…")`), so `localizedDescription` would show that.
    var viewerMessage: String {
        switch self as? AuroraError {
        case .Network(let message), .Database(let message), .Credentials(let message),
             .NotFound(let message), .Other(let message), .Parental(_, let message):
            return message
        case nil:
            return localizedDescription
        }
    }
}

extension Error {
    /// The parental gate's refusal, when that is what this is.
    var refusal: Refusal? {
        if case .Parental(let refusal, _) = self as? AuroraError { return refusal }
        return nil
    }

    /// Refused for want of the parental PIN: a lock, the day's time, or a guarded
    /// change. Entering it and trying again is the answer (Windows' `invoke` wrapper).
    var wantsParentalPin: Bool {
        switch refusal {
        case .locked, .timeUp, .pinRequired: true
        case .notAllowed, nil: false
        }
    }
}
