// The review's stylesheet alone (dist/review.css), for a host that shows SpecifyReview without the diff
// components. dist/specify-ui.css also carries StepDiff.css, whose page-level rule (html body: overflow,
// user-select, background) is meant for a diff page and changes any other page that loads it.
import { copyFileSync } from 'node:fs';
copyFileSync('src/specify/PmiLab.css', 'dist/review.css');
