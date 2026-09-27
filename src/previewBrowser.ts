import type {PreviewBrowser} from './types';

/** User previews use the system browser; managed Shot2AI status is irrelevant. */
export function shot2ai(_browser:PreviewBrowser|null):string{
 return 'Opens in your default browser.';
}
