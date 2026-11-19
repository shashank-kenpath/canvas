export interface Env {
  DB: D1Database;
  ROOMS: DurableObjectNamespace;
  ASSETS: R2Bucket;
  ALLOWED_DOMAINS: string;
  GOOGLE_CLIENT_ID: string;
  GOOGLE_CLIENT_SECRET?: string;
  /** Tests point this at a fake; production leaves it unset. */
  GOOGLE_BASE?: string;
}

export type Role = "owner" | "editor" | "viewer";

/** What a board room knows about a connected person. */
export interface Person {
  id: string;
  name: string;
  email: string;
  color: string;
}

export interface User {
  id: string;
  email: string;
  name: string;
  picture: string | null;
}
