import * as z from "zod";
import { AccountType, accountTypeSchema } from "./constants";

export const trackingModeSchema = z.enum(["TRANSACTIONS", "HOLDINGS", "NOT_SET"]);

export const newAccountSchema = z
  .object({
    id: z.string().uuid().optional(),
    name: z
      .string()
      .min(2, {
        message: "Name must be at least 2 characters.",
      })
      .max(50, {
        message: "Name must not be longer than 50 characters.",
      }),
    group: z.string().optional(),
    isDefault: z.boolean().optional(),
    isActive: z.boolean().optional(),
    isArchived: z.boolean().optional().default(false),
    accountType: accountTypeSchema,
    currency: z.string({ required_error: "Please select a currency." }),
    trackingMode: trackingModeSchema.optional().default("NOT_SET"),
    meta: z.string().nullable().optional(),
  })
  .refine(
    (data) => data.accountType !== AccountType.CREDIT_CARD || data.trackingMode !== "HOLDINGS",
    {
      message: "Credit card accounts cannot use holdings tracking mode.",
      path: ["trackingMode"],
    },
  );
