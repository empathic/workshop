/**
 * Fun activity verbs for when Claude is working.
 * Used by baud meter and activity indicators.
 */
export const activityVerbs = [
  'Pondering',
  'Cogitating',
  'Ruminating',
  'Musing',
  'Deliberating',
  'Contemplating',
  'Brewing',
  'Conjuring',
  'Scheming',
  'Plotting',
  'Crafting',
  'Forging',
  'Weaving',
  'Spinning',
  'Channeling',
  'Summoning',
  'Invoking',
  'Manifesting',
  'Synthesizing',
  'Distilling',
  'Percolating',
  'Fermenting',
  'Incubating',
  'Gestating',
  'Hatching',
  'Concocting',
  'Devising',
  'Dreaming',
  'Imagining',
  'Envisioning',
  'Computing',
  'Processing',
  'Crunching',
  'Parsing',
  'Decoding',
  'Analyzing',
  'Dissecting',
  'Examining',
  'Probing',
  'Scanning',
  'Exploring',
  'Traversing',
  'Navigating',
  'Charting',
  'Mapping',
  'Assembling',
  'Composing',
  'Arranging',
  'Orchestrating',
  'Conducting'
];

/** Pick a random activity verb */
export function randomVerb(): string {
  return activityVerbs[Math.floor(Math.random() * activityVerbs.length)];
}
