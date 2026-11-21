// The editor on a page with a strict Content Security Policy (see csp.html).
import { createEditor } from './rustaco.js';

window.violations = [];
document.addEventListener('securitypolicyviolation', (e) => window.violations.push(`${e.violatedDirective} ${e.blockedURI}`));
window.editor = await createEditor(document.getElementById('root'), { value: 'fn main() {}\n', language: 'rust' });
