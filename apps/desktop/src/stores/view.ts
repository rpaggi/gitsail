// View state (SAD §17). Purely presentational, has no meaning outside this
// window/session: in-flight flags and the last error to show, driving the
// visible "operation in progress" state the frontend-engineering-practices
// skill requires for every user-visible async action.

import { defineStore } from "pinia";

import type { ErrorPayload } from "../services/errors";

export const useViewStore = defineStore("view", {
  state: () => ({
    isOpeningRepository: false,
    isRefreshingStatus: false,
    lastError: null as ErrorPayload | null,
  }),
});
