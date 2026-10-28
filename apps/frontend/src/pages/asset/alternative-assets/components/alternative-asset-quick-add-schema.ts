import type { LoanSetup } from "@/adapters/shared/alternative-assets";
import { AlternativeAssetKind } from "@/lib/types";
import { formatDateISO } from "@/lib/utils";
import * as z from "zod";
import type { LoanInterestMethod, LoanPaymentFrequency } from "../lib/loan-events";

// Metal types for precious metals
export const METAL_TYPES = [
  { value: "gold", label: "黄金" },
  { value: "silver", label: "白银" },
  { value: "platinum", label: "铂金" },
  { value: "palladium", label: "钯金" },
] as const;

// Weight units for precious metals
export const WEIGHT_UNITS = [
  { value: "oz", label: "金衡盎司（oz）" },
  { value: "g", label: "克（g）" },
  { value: "kg", label: "千克（kg）" },
] as const;

// Liability types. Credit cards are accounts, so new liabilities can't use that type.
export const LIABILITY_TYPES = [
  { value: "mortgage", label: "房贷" },
  { value: "auto_loan", label: "车贷" },
  { value: "student_loan", label: "助学贷款" },
  { value: "personal_loan", label: "个人贷款" },
  { value: "heloc", label: "住房净值信用额度（HELOC）" },
  { value: "other", label: "其他" },
] as const;

// Asset type options for the type selector
export const ASSET_KIND_OPTIONS = [
  { value: AlternativeAssetKind.PROPERTY, label: "房产" },
  { value: AlternativeAssetKind.VEHICLE, label: "车辆" },
  { value: AlternativeAssetKind.COLLECTIBLE, label: "收藏品" },
  { value: AlternativeAssetKind.PRECIOUS_METAL, label: "贵金属" },
  { value: AlternativeAssetKind.LIABILITY, label: "负债" },
  { value: AlternativeAssetKind.OTHER, label: "其他" },
] as const;

export const liabilityQuickAddSchema = z
  .object({
    originalAmount: z.coerce.number().finite().positive().optional(),
    currentBalance: z.coerce.number().finite().min(0).optional(),
    originationDate: z.date().optional(),
    balanceDate: z.date(),
    loanTermMonths: z.coerce.number().finite().int().positive().max(1200).optional(),
    interestRate: z.coerce.number().finite().min(0).max(100).optional(),
  })
  .superRefine((values, context) => {
    if (!values.originalAmount && values.currentBalance === undefined) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["currentBalance"],
        message: "asset:quickAdd.validation.invalid",
      });
    }
    if (values.originationDate && values.balanceDate < values.originationDate) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["balanceDate"],
        message: "asset:quickAdd.validation.balance_date_before_origination",
      });
    }
  });

// Zod schema for the quick add form
export const alternativeAssetQuickAddSchema = z
  .object({
    // Asset type
    kind: z.enum([
      AlternativeAssetKind.PROPERTY,
      AlternativeAssetKind.VEHICLE,
      AlternativeAssetKind.COLLECTIBLE,
      AlternativeAssetKind.PRECIOUS_METAL,
      AlternativeAssetKind.LIABILITY,
      AlternativeAssetKind.OTHER,
    ]),

    // Common fields
    name: z.string().min(1, "名称为必填项").max(100, "名称不能超过 100 个字符"),
    currency: z.string().min(1, "币种为必填项"),
    quantity: z.coerce
      .number({
        required_error: "请输入有效数量。",
        invalid_type_error: "数量必须为数字。",
      })
      .positive("数量必须大于 0"),
    currentValue: z.coerce
      .number({
        required_error: "请输入有效金额。",
        invalid_type_error: "金额必须为数字。",
      })
      .min(0, "金额不能为负数"),
    valueDate: z.date({
      required_error: "估值日期为必填项",
    }),

    // Property-specific: has mortgage checkbox
    hasMortgage: z.boolean().optional(),

    // Precious metal-specific fields
    metalType: z.enum(["gold", "silver", "platinum", "palladium"]).optional(),
    weightUnit: z.enum(["oz", "g", "kg"]).optional(),

    // Liability-specific fields
    liabilityType: z
      .enum(["mortgage", "auto_loan", "student_loan", "personal_loan", "heloc", "other"])
      .optional(),
    linkedAssetId: z.string().optional(),
  })
  .refine(
    (data) => {
      // Precious metals require metal type and weight unit
      if (data.kind === AlternativeAssetKind.PRECIOUS_METAL) {
        return !!data.metalType && !!data.weightUnit;
      }
      return true;
    },
    {
      message: "Metal type and unit are required for precious metals",
      path: ["metalType"],
    },
  );

export type AlternativeAssetQuickAddFormValues = z.infer<typeof alternativeAssetQuickAddSchema>;

// Default form values
export const getDefaultFormValues = (): AlternativeAssetQuickAddFormValues => ({
  kind: AlternativeAssetKind.PROPERTY,
  name: "",
  currency: "USD",
  quantity: 1,
  currentValue: 0,
  valueDate: new Date(),
  hasMortgage: false,
  metalType: undefined,
  weightUnit: "oz",
  liabilityType: undefined,
  linkedAssetId: undefined,
});

/** What the quick-add form holds for a new loan. */
export interface QuickAddLoanInput {
  purchasePrice?: string;
  purchaseDate?: Date;
  interestRate?: string;
  automaticSchedule?: boolean;
  paymentFrequency?: LoanPaymentFrequency;
  interestMethod?: LoanInterestMethod;
  loanTerm?: string;
  loanTermMonths?: string;
  firstPaymentDate?: Date;
}

const entered = (value?: string) => {
  const number = value?.trim() ? Number(value) : NaN;
  return Number.isFinite(number) ? number : undefined;
};

/**
 * A new loan as entered. The backend checks it, derives the schedule and solves
 * the payment; a manual loan keeps only its amounts, as before.
 */
export function quickAddLoanSetup(input: QuickAddLoanInput): LoanSetup {
  const amounts = {
    originalAmount: entered(input.purchasePrice),
    interestRate: entered(input.interestRate),
  };
  if (input.automaticSchedule === false) return amounts;
  const months = (entered(input.loanTerm) ?? 0) * 12 + (entered(input.loanTermMonths) ?? 0);
  return {
    ...amounts,
    originationDate: input.purchaseDate ? formatDateISO(input.purchaseDate) : undefined,
    schedule: {
      frequency: input.paymentFrequency ?? "monthly",
      interestMethod: input.interestMethod ?? "nominal_periodic",
      amortizationMonths: months > 0 ? months : undefined,
      firstPaymentDate: input.firstPaymentDate ? formatDateISO(input.firstPaymentDate) : undefined,
    },
  };
}
