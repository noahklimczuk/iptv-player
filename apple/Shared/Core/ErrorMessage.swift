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
             .NotFound(let message), .Other(let message):
            return message
        case nil:
            return localizedDescription
        }
    }
}
