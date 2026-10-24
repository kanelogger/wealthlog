import { createI18n } from "./create-i18n";
import { recordMissingKey } from "./missing-keys";

const saveMissing = import.meta.env.MODE === "test";

const i18n = createI18n({
  saveMissing,
  onMissingKey: saveMissing ? recordMissingKey : undefined,
});

export default i18n;
