/** The specify-core session API spoken by this app, independent of its release version. */
export const SESSION_API = 2;

export class SessionApiMismatch extends Error {
  constructor(api: number) {
    super(`This Specify page speaks session API ${SESSION_API}, but specify-core speaks ${api}: update specify-core or the app`);
    this.name = 'SessionApiMismatch';
  }
}

/** specify-core 0.2.0 omits api (its contract is 1); any version but this app's is incompatible. */
export function requireSessionApi(api: number | undefined): void {
  if ((api ?? 1) !== SESSION_API) throw new SessionApiMismatch(api ?? 1);
}
