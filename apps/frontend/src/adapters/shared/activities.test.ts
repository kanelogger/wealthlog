import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  logger: {
    error: vi.fn(),
  },
}));

vi.mock("./platform", () => ({
  invoke: mocks.invoke,
  logger: mocks.logger,
}));

import { createActivity, saveActivities, updateActivity } from "./activities";

const baseActivity = {
  accountId: "account-1",
  activityType: "CREDIT",
  subtype: "REFUND",
  activityDate: "2026-08-11T00:00:00.000Z",
  currency: "USD",
};

describe("activity metadata serialization", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.invoke.mockResolvedValue({});
  });

  it("serializes object metadata for creates without mutating the caller payload", async () => {
    const activity = {
      ...baseActivity,
      metadata: { flow: { is_external: true } },
    };

    await createActivity(activity);

    expect(activity.metadata).toEqual({ flow: { is_external: true } });
    expect(mocks.invoke).toHaveBeenCalledWith("create_activity", {
      activity: {
        ...baseActivity,
        metadata: '{"flow":{"is_external":true}}',
      },
    });
  });

  it("serializes object metadata for updates", async () => {
    await updateActivity({
      id: "activity-1",
      ...baseActivity,
      metadata: { raw_type: "merchant_refund", flow: { is_external: true } },
    });

    expect(mocks.invoke).toHaveBeenCalledWith("update_activity", {
      activity: {
        id: "activity-1",
        ...baseActivity,
        metadata: '{"raw_type":"merchant_refund","flow":{"is_external":true}}',
      },
    });
  });

  it("normalizes metadata in bulk creates and updates while preserving JSON strings", async () => {
    await saveActivities({
      creates: [{ ...baseActivity, metadata: { flow: { is_external: true } } }],
      updates: [
        {
          id: "activity-1",
          ...baseActivity,
          metadata: '{"flow":{"is_external":false}}',
        },
      ],
    });

    expect(mocks.invoke).toHaveBeenCalledWith("save_activities", {
      request: {
        creates: [
          {
            ...baseActivity,
            metadata: '{"flow":{"is_external":true}}',
          },
        ],
        updates: [
          {
            id: "activity-1",
            ...baseActivity,
            metadata: '{"flow":{"is_external":false}}',
          },
        ],
        deleteIds: [],
      },
    });
  });
});
