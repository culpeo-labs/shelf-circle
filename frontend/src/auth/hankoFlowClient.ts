import { HANKO_API_URL } from '../config';

/**
 * Hanko's public API is driven by a generic state machine (the "Flow API"):
 * every response names the current state, the actions available from it, and
 * the inputs each action needs. Submitting an action POSTs to that action's
 * `href` and gets back the next state. We deliberately don't hardcode state
 * or action names here — the exact steps (passcode vs. passkey vs. password)
 * depend on how the Hanko Cloud project is configured, and can change from
 * the dashboard without an app update. `FlowScreen` renders whatever this
 * returns.
 *
 * On success, Hanko returns the session JWT in an `X-Auth-Token` response
 * header (in addition to a cookie, which we ignore — RN has no cookie jar by
 * default and we don't need one).
 *
 * Docs: https://docs.hanko.io/using-the-api/understanding-the-flow-api
 */

export interface FlowInput {
  name: string;
  type: string;
  value?: unknown;
  min_length?: number;
  max_length?: number;
  required?: boolean;
  hidden?: boolean;
  error?: { code: string; message: string };
}

export interface FlowAction {
  action: string;
  description?: string;
  href: string;
  inputs: Record<string, FlowInput>;
}

export interface FlowState {
  name: string;
  status: number;
  csrf_token?: string;
  actions: Record<string, FlowAction>;
  error?: { code: string; message: string };
  payload?: unknown;
}

export interface FlowResult extends FlowState {
  /** Present once the flow completes and issues a session. */
  authToken?: string;
}

async function post(url: string, body: unknown): Promise<FlowResult> {
  let response: Response;
  try {
    response = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
      body: JSON.stringify(body),
    });
  } catch {
    throw new Error('Could not reach the sign-in service. Check your connection and try again.');
  }

  const text = await response.text();
  const data: FlowState = text
    ? JSON.parse(text)
    : { name: 'unknown', status: response.status, actions: {} };

  const authToken = response.headers.get('X-Auth-Token') ?? undefined;
  return { ...data, authToken };
}

function resolveHref(href: string): string {
  return href.startsWith('http') ? href : `${HANKO_API_URL}${href.startsWith('/') ? '' : '/'}${href}`;
}

/** Starts a flow, e.g. `start('/login')`. An empty body is fine — Hanko ignores it. */
export function startFlow(path: string): Promise<FlowResult> {
  return post(`${HANKO_API_URL}${path.startsWith('/') ? '' : '/'}${path}`, {});
}

/** Submits one of `state`'s actions with the given input values. */
export function submitFlowAction(
  state: FlowState,
  actionName: string,
  inputData: Record<string, string>
): Promise<FlowResult> {
  const action = state.actions[actionName];
  if (!action) {
    throw new Error(`"${state.name}" has no action "${actionName}" (it may have expired — try again).`);
  }
  return post(resolveHref(action.href), { input_data: inputData, csrf_token: state.csrf_token });
}
