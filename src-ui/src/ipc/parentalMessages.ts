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
/** `profiles::ADULT_NEEDS_PIN`: adult categories were to be shown with no parental PIN. */
export const ADULT_NEEDS_PIN_MESSAGE =
  'Set a parental PIN before showing adult categories, so that only you can show them.';
/** `AppError::TimeUp`: today's viewing time for the profile watching is used up. */
export const TIME_UP_MESSAGE =
  "Today's viewing time is used up. Enter the parental PIN to keep watching.";
