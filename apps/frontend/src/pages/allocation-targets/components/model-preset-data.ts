export interface ModelPreset {
  id: string;
  taxonomyId: string;
  name: string;
  description: string;
  risk: string;
  featured?: boolean;
  weights: Record<string, number>; // 0-100, keyed by taxonomy category id
}

export const BUILT_IN_PRESETS: ModelPreset[] = [
  // Asset Classes
  {
    id: "balanced_60_40",
    taxonomyId: "asset_classes",
    name: "均衡型 60 / 40",
    description: "经典股债组合",
    risk: "Moderate",
    featured: true,
    weights: { EQUITY: 60, FIXED_INCOME: 40 },
  },
  {
    id: "growth_80_20",
    taxonomyId: "asset_classes",
    name: "成长型 80 / 20",
    description: "以股票增长为主，债券平衡波动",
    risk: "Aggressive",
    featured: true,
    weights: { EQUITY: 80, FIXED_INCOME: 20 },
  },
  {
    id: "all_weather",
    taxonomyId: "asset_classes",
    name: "全天候组合",
    description: "分散配置，适应不同市场环境",
    risk: "Conservative",
    featured: true,
    weights: { EQUITY: 30, FIXED_INCOME: 55, COMMODITIES: 15 },
  },
  {
    id: "income_20_80",
    taxonomyId: "asset_classes",
    name: "收益型 20 / 80",
    description: "以债券为主，侧重资产保全",
    risk: "Conservative",
    weights: { EQUITY: 20, FIXED_INCOME: 80 },
  },
  {
    id: "conservative_growth_40_60",
    taxonomyId: "asset_classes",
    name: "稳健成长型 40 / 60",
    description: "偏稳健的股债组合",
    risk: "Conservative",
    weights: { EQUITY: 40, FIXED_INCOME: 60 },
  },
  {
    id: "aggressive_90_10",
    taxonomyId: "asset_classes",
    name: "进取型 90 / 10",
    description: "以股票为主的成长组合",
    risk: "Aggressive",
    weights: { EQUITY: 90, FIXED_INCOME: 10 },
  },
  {
    id: "permanent_portfolio",
    taxonomyId: "asset_classes",
    name: "永久组合",
    description: "股票、债券、现金与大宗商品等权配置",
    risk: "Conservative",
    weights: { EQUITY: 25, FIXED_INCOME: 25, CASH: 25, COMMODITIES: 25 },
  },

  // Industries GICS
  {
    id: "gics_sp500_weight",
    taxonomyId: "industries_gics",
    name: "标普 500",
    description: "标普 500 行业权重（2026 年 5 月）",
    risk: "Moderate",
    weights: {
      "45": 39,
      "40": 11,
      "50": 10,
      "25": 10,
      "35": 8,
      "20": 8,
      "30": 5,
      "10": 3,
      "55": 2,
      "15": 2,
      "60": 2,
    },
  },
  {
    id: "gics_equal_weight",
    taxonomyId: "industries_gics",
    name: "行业等权配置",
    description: "在 11 个 GICS 行业中等权配置",
    risk: "Moderate",
    weights: {
      "10": 9,
      "15": 9,
      "20": 9,
      "25": 9,
      "30": 9,
      "35": 9,
      "40": 9,
      "45": 10,
      "50": 9,
      "55": 9,
      "60": 9,
    },
  },
  {
    id: "gics_defensive",
    taxonomyId: "industries_gics",
    name: "防御型股票组合",
    description: "侧重医疗保健、必需消费与公用事业",
    risk: "Conservative",
    weights: { "35": 40, "30": 35, "55": 25 },
  },

  // Risk Category
  {
    id: "risk_conservative",
    taxonomyId: "risk_category",
    name: "稳健型",
    description: "以低风险资产为主，少量配置高风险资产",
    risk: "Conservative",
    weights: { LOW: 70, MEDIUM: 25, HIGH: 5 },
  },
  {
    id: "risk_balanced",
    taxonomyId: "risk_category",
    name: "均衡型",
    description: "以中风险资产为核心，搭配低风险资产",
    risk: "Moderate",
    weights: { LOW: 30, MEDIUM: 50, HIGH: 20 },
  },
  {
    id: "risk_aggressive",
    taxonomyId: "risk_category",
    name: "进取型",
    description: "以增长为目标，主要配置高风险资产",
    risk: "Aggressive",
    weights: { LOW: 10, MEDIUM: 30, HIGH: 60 },
  },

  // Regions
  {
    id: "regions_global_cap",
    taxonomyId: "regions",
    name: "全球市值配置",
    description: "近似全球市值权重（2026 年 4 月）",
    risk: "Moderate",
    weights: { R20: 67, R10: 16, R30: 14, R50: 2, R40: 1 },
  },
  {
    id: "regions_international_proxy",
    taxonomyId: "regions",
    name: "国际股票参考组合",
    description: "按大洲配置，近似美国以外的股票市场",
    risk: "Moderate",
    weights: { R10: 42, R30: 40, R20: 13, R50: 3, R40: 2 },
  },
  {
    id: "regions_equal_weight",
    taxonomyId: "regions",
    name: "地区等权配置",
    description: "在五大洲等权配置",
    risk: "Aggressive",
    weights: { R10: 20, R20: 20, R30: 20, R40: 20, R50: 20 },
  },
];
