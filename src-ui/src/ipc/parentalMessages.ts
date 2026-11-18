/**
 * The host's parental refusals, word for word (`AppError::Locked`,
 * `AppError::ParentalPinRequired` in `aurora-app/src/error.rs`).
 *
 * Their own module because both the transport (`index.ts`, which answers them with the
 * PIN dialog) and the mock (which has to refuse in the same words) need them, and the
 * mock is imported by the transport.
 */
export const LOCKED_MESSAGE = 'This is locked. Enter the parental PIN to watch it.';
export const PIN_REQUIRED_MESSAGE = 'Enter the parental PIN to change this.';
