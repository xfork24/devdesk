// The one sanctioned loose-record hatch: kernel JSON rows are structural data
// owned by the RPC contract, not the UI.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type Row = Record<string, any>;
export type RequestOptions = {
  signal?: AbortSignal;
  fresh?: boolean;
  prefetch?: boolean;
  shared?: boolean;
};
export type Request = (
  method: string,
  params?: Row,
  options?: RequestOptions,
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
) => Promise<any>;

export class RpcResponseTimeout extends Error {}

export type ResourceVersion = {
  resource: { kind: string; id: string };
  revision: number;
};

export type HostVersion = { instance_id: string; revision: number };
