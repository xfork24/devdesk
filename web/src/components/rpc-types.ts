// The one sanctioned loose-record hatch: kernel JSON rows are structural data
// owned by the RPC contract, not the UI.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type Row = Record<string, any>;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type Request = (method: string, params?: Row) => Promise<any>;

export type ResourceVersion = {
  resource: { kind: string; id: string };
  revision: number;
};
