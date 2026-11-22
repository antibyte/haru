import type { Language } from '../../ipc/types';
import { de, type Strings } from './de';
import { en } from './en';
import { es } from './es';
import { fr } from './fr';
import { ja } from './ja';
import { ko } from './ko';
import { pt } from './pt';
import { ru } from './ru';
import { zh } from './zh';

/**
 * UI string tables, one for every supported language. Unsupported language codes resolve to English in strings.ts
 * before a table is looked up.
 */
export const TABLES: Record<Language, Strings> = { de, en, es, fr, ja, ko, pt, ru, zh };
