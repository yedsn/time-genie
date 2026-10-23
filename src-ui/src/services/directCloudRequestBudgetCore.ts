export type DirectCloudRequestEvent = {
  kind: "mutation" | "query" | "refresh";
  key: string;
  source?: string;
};

export class DirectCloudRequestBudget {
  readonly events: DirectCloudRequestEvent[] = [];

  mutation<T>(key: string, run: () => T): T {
    this.events.push({ kind: "mutation", key });
    return run();
  }

  query<T>(key: string, source: string, run: () => T): T {
    this.events.push({ kind: "query", key, source });
    return run();
  }

  refresh(key: string, source: string) {
    this.events.push({ kind: "refresh", key, source });
  }

  count(kind: DirectCloudRequestEvent["kind"], key?: string) {
    return this.events.filter((event) => event.kind === kind && (!key || event.key === key)).length;
  }
}
