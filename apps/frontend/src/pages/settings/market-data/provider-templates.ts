/** Quick-start templates for custom provider source configuration. */

export interface ProviderTemplate {
  name: string;
  description: string;
  format: "json" | "html" | "html_table" | "csv";
  url: string;
  pricePath: string;
  datePath?: string;
  dateFormat?: string;
  currencyPath?: string;
  openPath?: string;
  highPath?: string;
  lowPath?: string;
  volumePath?: string;
  headers?: string;
  testSymbol: string;
}

export const LATEST_TEMPLATES: ProviderTemplate[] = [
  {
    name: "Vanguard",
    description: "职场基金价格（基金代码）",
    format: "json",
    url: "https://workplace.vanguard.com/investments/product-details/fund/api/price-distribution/fundPrice/{SYMBOL}?startDate={DATE:%Y-01-01}&endDate={TODAY}",
    pricePath: "$.body.fundPrice.content[-1:].price",
    datePath: "$.body.fundPrice.content[-1:].effectiveDate",
    dateFormat: "%Y-%m-%d",
    currencyPath: "$.body.fundPrice.content[-1:].currencyCode",
    testSymbol: "M219",
  },
  {
    name: "CoinGecko",
    description: "免费加密资产行情（使用币种 ID，如 bitcoin、ethereum）",
    format: "json",
    url: "https://api.coingecko.com/api/v3/simple/price?ids={SYMBOL}&vs_currencies={currency}",
    pricePath: "$.{SYMBOL}.{currency}",
    testSymbol: "bitcoin",
  },
  {
    name: "ExchangeRate API",
    description: "免费汇率数据",
    format: "json",
    url: "https://open.er-api.com/v6/latest/{SYMBOL}",
    pricePath: "$.rates.EUR",
    testSymbol: "USD",
  },
  {
    name: "FT.com",
    description: "伦敦证券交易所 ETF 与股票",
    format: "html",
    url: "https://markets.ft.com/data/etfs/tearsheet/summary?s={SYMBOL}:LSE:GBX",
    pricePath: ".mod-tearsheet-overview__quote .mod-ui-data-list__value",
    testSymbol: "3SBA",
  },
  {
    name: "Euronext",
    description: "欧洲基金与股票（ISIN-MIC）",
    format: "html",
    url: "https://live.euronext.com/en/ajax/getDetailedQuote/{SYMBOL}",
    pricePath: "#header-instrument-price",
    testSymbol: "NL0015000GU4-XAMS",
  },
  {
    name: "Twelve Data",
    description: "股票、加密资产与外汇（在请求头中设置 API 密钥）",
    format: "json",
    url: "https://api.twelvedata.com/price?symbol={SYMBOL}",
    pricePath: "$.price",
    headers: '{"Authorization": "apikey YOUR_API_KEY"}',
    testSymbol: "AAPL",
  },
  {
    name: "Borsa Italiana",
    description: "意大利债券与股票",
    format: "html",
    url: "https://www.borsaitaliana.it/borsa/obbligazioni/mot/btp/scheda/{SYMBOL}.html?lang=en",
    pricePath: ".summary-value strong",
    testSymbol: "IT0001174611",
  },
];

export const HISTORICAL_TEMPLATES: ProviderTemplate[] = [
  {
    name: "Vanguard",
    description: "职场基金历史价格（基金代码）",
    format: "json",
    url: "https://workplace.vanguard.com/investments/product-details/fund/api/price-distribution/fundPrice/{SYMBOL}?startDate={FROM}&endDate={TO}",
    pricePath: "$.body.fundPrice.content[*].price",
    datePath: "$.body.fundPrice.content[*].effectiveDate",
    dateFormat: "%Y-%m-%d",
    currencyPath: "$.body.fundPrice.content[*].currencyCode",
    testSymbol: "M219",
  },
  {
    name: "Twelve Data (JSON)",
    description: "股票、加密资产与外汇（在请求头中设置 API 密钥）",
    format: "json",
    url: "https://api.twelvedata.com/time_series?symbol={SYMBOL}&interval=1day&start_date={FROM}&end_date={TO}&format=JSON",
    pricePath: "$.values[*].close",
    datePath: "$.values[*].datetime",
    openPath: "$.values[*].open",
    highPath: "$.values[*].high",
    lowPath: "$.values[*].low",
    volumePath: "$.values[*].volume",
    headers: '{"Authorization": "apikey YOUR_API_KEY"}',
    testSymbol: "AAPL",
  },
  {
    name: "Twelve Data (CSV)",
    description: "股票、加密资产与外汇（在请求头中设置 API 密钥）",
    format: "csv",
    url: "https://api.twelvedata.com/time_series?symbol={SYMBOL}&interval=1day&start_date={FROM}&end_date={TO}&format=CSV",
    pricePath: "close",
    datePath: "datetime",
    openPath: "open",
    highPath: "high",
    lowPath: "low",
    volumePath: "volume",
    headers: '{"Authorization": "apikey YOUR_API_KEY"}',
    testSymbol: "AAPL",
  },
  {
    name: "FT.com",
    description: "伦敦证券交易所 ETF 与股票（HTML 表格）",
    format: "html_table",
    url: "https://markets.ft.com/data/etfs/tearsheet/historical?s={SYMBOL}:LSE:GBX",
    pricePath: "0:4",
    datePath: "0:0",
    openPath: "0:1",
    highPath: "0:2",
    lowPath: "0:3",
    volumePath: "0:5",
    testSymbol: "3SBA",
  },
  {
    name: "CoinGecko",
    description: "加密资产每日历史行情（使用币种 ID，如 bitcoin、ethereum）",
    format: "json",
    url: "https://api.coingecko.com/api/v3/coins/{SYMBOL}/market_chart?vs_currency={currency}&days=365&interval=daily",
    pricePath: "$.prices[*][1]",
    datePath: "$.prices[*][0]",
    volumePath: "$.total_volumes[*][1]",
    testSymbol: "bitcoin",
  },
];
