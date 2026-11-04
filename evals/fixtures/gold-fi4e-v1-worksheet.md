# Gold-corpus adjudication worksheet — bajan-2k4 HITL gate

Book: *Financial Intelligence for Entrepreneurs* (Berman & Knight, 2011) — 8 real-prose
slices (~1.3k chars each). Deterministic extractor proposals listed per episode;
for each episode, author the claim set the text SHOULD yield.

For each episode decide:
1. Which proposals are genuine claims (keep text verbatim, cite the exact span).
2. Which proposals are junk/not-claims (cross out).
3. Any claims the extractor MISSED (author new ones).
4. For every kept claim: hedge markers in the episode within its coverage
   (may/might/could/possibly/appears/seems/signals/estimates/reportedly/tentative/uncertain).

Oracle rule: gold labels are YOUR sign-off — no model-generated gold labels.

**Human sign-off: 2026-11-04.** The draft below was AI-assisted under the
user's explicit authorization to draft; the user reviewed the completed
adjudication and confirmed it in session chat, adopting these decisions as
gold labels (oracle rule satisfied: sign-off is human).

Adjudication policy: retain assertions, definitions, reported events, attributed
beliefs, and qualified predictions. Preserve their original wording and modality;
reported beliefs are not endorsed as facts. Exclude questions, headings,
directives, and passages serving only as navigation or rhetorical transitions.
Navigation means pointers to chapters or parts and instructions to keep reading;
exclude those pointers (#62, #67, #85, #94), recovering substantive embedded
assertions separately. Retain assertions about the authors’ teaching practice
(#1), instructional method (#37: definitions at first use), intended learning
outcomes (#27), and educational objectives (#91). These describe teaching or
learning rather than merely identifying a chapter or part.
Mixed question/assertion and heading/assertion proposals are crossed out and
replaced below with exact assertion spans.
A checked box means KEEP; strikethrough means CROSS OUT. Each retained claim
lists hedge tokens from the worksheet’s specified vocabulary within its own
span (case-insensitive whole words); `none` means none from that vocabulary.
Other qualifiers remain in the verbatim text. In particular, `seem`, `seemed`,
`probably`, and `maybe` are preserved but are not exact listed hedge tokens.
`could` and `estimates` are recorded lexically even where they express ability
or refer to accounting estimates rather than epistemic uncertainty.

Span convention: `source[start:end]` uses zero-based Unicode character offsets,
with an exclusive end, relative to the SOURCE TEXT paragraph of each episode.
Spans locate the verbatim proposal text, excluding Markdown and annotations.

## fi4e-gold-ep00 (15 proposals)

SOURCE TEXT (1272 chars):

1 What Is Financial Intelligence? Since we teach finance for a living, we’ll begin this book in the manner of teachers everywhere: by asking questions. Do you know whether you will have enough cash to make payroll next month? How about the month after that? If you’re running a start-up, do you know your burn rate—that is, how fast you are going through your cash? Do you know how profitable your company’s products or services really are? Do you know that you can be running a profitable business and still run out of cash? If you’re thinking about buying a new piece of equipment—a truck, a computer system, a machine—do you know how to figure the likely return on your investment? Many entrepreneurs can’t answer yes to questions like these. The reason is that they haven’t yet acquired the necessary financial intelligence. Note that word: acquired. Financial intelligence, as we use the term, isn’t some innate ability that you either have or don’t have. Granted, some people are better at numbers than others, and a few legendary folks seem to have an intuitive grasp of finance that eludes everybody else. But that’s not what we’re talking about here. For most businesspeople—ourselves included—financial intelligence is simply a set of skills that can be learned.

EXTRACTOR PROPOSALS:

- [ ] #0: ~~1 What Is Financial Intelligence?~~
  - Decision: CROSS OUT — Chapter heading phrased as a question.
  - Exact span: `source[0:33]`.
- [x] #1: Since we teach finance for a living, we’ll begin this book in the manner of teachers everywhere: by asking questions.
  - Exact span: `source[34:151]`.
  - Hedge markers: none.
- [ ] #2: ~~Do you know whether you will have enough cash to make payroll next month?~~
  - Decision: CROSS OUT — Question; no asserted answer.
  - Exact span: `source[152:225]`.
- [ ] #3: ~~How about the month after that?~~
  - Decision: CROSS OUT — Elliptical follow-up question.
  - Exact span: `source[226:257]`.
- [ ] #4: ~~If you’re running a start-up, do you know your burn rate—that is, how fast you are going through your cash?~~
  - Decision: CROSS OUT — Question; the embedded definition is recovered below.
  - Exact span: `source[258:365]`.
- [ ] #5: ~~Do you know how profitable your company’s products or services really are?~~
  - Decision: CROSS OUT — Question; no asserted answer.
  - Exact span: `source[366:440]`.
- [ ] #6: ~~Do you know that you can be running a profitable business and still run out of cash?~~
  - Decision: CROSS OUT — Question; the embedded assertion is recovered below.
  - Exact span: `source[441:525]`.
- [ ] #7: ~~If you’re thinking about buying a new piece of equipment—a truck, a computer system, a machine—do you know how to figure the likely return on your investment?~~
  - Decision: CROSS OUT — Question; no asserted answer.
  - Exact span: `source[526:684]`.
- [x] #8: Many entrepreneurs can’t answer yes to questions like these.
  - Exact span: `source[685:745]`.
  - Hedge markers: none.
- [x] #9: The reason is that they haven’t yet acquired the necessary financial intelligence.
  - Exact span: `source[746:828]`.
  - Hedge markers: none.
- [ ] #10: ~~Note that word: acquired.~~
  - Decision: CROSS OUT — Reading directive.
  - Exact span: `source[829:854]`.
- [x] #11: Financial intelligence, as we use the term, isn’t some innate ability that you either have or don’t have.
  - Exact span: `source[855:960]`.
  - Hedge markers: none.
- [x] #12: Granted, some people are better at numbers than others, and a few legendary folks seem to have an intuitive grasp of finance that eludes everybody else.
  - Exact span: `source[961:1113]`.
  - Hedge markers: none.
- [ ] #13: ~~But that’s not what we’re talking about here.~~
  - Decision: CROSS OUT — Rhetorical scope transition.
  - Exact span: `source[1114:1159]`.
- [x] #14: For most businesspeople—ourselves included—financial intelligence is simply a set of skills that can be learned.
  - Exact span: `source[1160:1272]`.
  - Hedge markers: none.


ADDITIONAL / RECOVERED CLAIMS:

- [x] M4: your burn rate—that is, how fast you are going through your cash
  - Exact span: `source[300:364]`.
  - Coverage: Embedded definition in question #4; includes both the term burn rate and its explanation.
  - Hedge markers: none.

- [x] M6: you can be running a profitable business and still run out of cash
  - Exact span: `source[458:524]`.
  - Coverage: Embedded assertion in question #6.
  - Hedge markers: none.

## fi4e-gold-ep01 (14 proposals)

SOURCE TEXT (1233 chars):

People who work in finance pick up these skills early on and for the rest of their careers are able to talk with one another in a specialized language that can sound like Greek to the uninitiated. Most senior executives of large companies (not all) either come out of finance or learn the skills during their rise to the top, just because it’s tough to run a big business unless you know what the financial folks are saying. But how about you? Nobody gave you an exam in finance when you decided to start a business. You probably didn’t launch your company just so that you could work with numbers. You may have had an accounting class in high school or college, but that isn’t enough to prepare you to manage a business. So you may never have had the chance to pick up financial skills. But now is the time. You may be a great salesperson or an inspired engineer. You may be terrific with customers and employees. Your concept for a company is probably fantastic. But if you don’t know finance, you’re operating at a disadvantage in the world of business. Fundamentally, financial intelligence boils down to three distinct skill sets. When you finish the book, you should be competent in all of them. Let’s look at each one in turn.

EXTRACTOR PROPOSALS:

- [x] #15: People who work in finance pick up these skills early on and for the rest of their careers are able to talk with one another in a specialized language that can sound like Greek to the uninitiated.
  - Exact span: `source[0:196]`.
  - Hedge markers: none.
- [x] #16: Most senior executives of large companies (not all) either come out of finance or learn the skills during their rise to the top, just because it’s tough to run a big business unless you know what the financial folks are saying.
  - Exact span: `source[197:424]`.
  - Hedge markers: none.
- [ ] #17: ~~But how about you? Nobody gave you an exam in finance when you decided to start a business.~~
  - Decision: CROSS OUT — Mixed rhetorical question and assertion; clean assertion recovered below.
  - Exact span: `source[425:516]`.
- [x] #18: You probably didn’t launch your company just so that you could work with numbers.
  - Exact span: `source[517:598]`.
  - Hedge markers: `could` at `source[574:579]`.
- [x] #19: You may have had an accounting class in high school or college, but that isn’t enough to prepare you to manage a business.
  - Exact span: `source[599:721]`.
  - Hedge markers: `may` at `source[603:606]`.
- [x] #20: So you may never have had the chance to pick up financial skills.
  - Exact span: `source[722:787]`.
  - Hedge markers: `may` at `source[729:732]`.
- [ ] #21: ~~But now is the time.~~
  - Decision: CROSS OUT — Exhortation without a substantive claim.
  - Exact span: `source[788:808]`.
- [x] #22: You may be a great salesperson or an inspired engineer.
  - Exact span: `source[809:864]`.
  - Hedge markers: `may` at `source[813:816]`.
- [x] #23: You may be terrific with customers and employees.
  - Exact span: `source[865:914]`.
  - Hedge markers: `may` at `source[869:872]`.
- [x] #24: Your concept for a company is probably fantastic.
  - Exact span: `source[915:964]`.
  - Hedge markers: none.
- [x] #25: But if you don’t know finance, you’re operating at a disadvantage in the world of business.
  - Exact span: `source[965:1056]`.
  - Hedge markers: none.
- [x] #26: Fundamentally, financial intelligence boils down to three distinct skill sets.
  - Exact span: `source[1057:1135]`.
  - Hedge markers: none.
- [x] #27: When you finish the book, you should be competent in all of them.
  - Exact span: `source[1136:1201]`.
  - Hedge markers: none.
- [ ] #28: ~~Let’s look at each one in turn.~~
  - Decision: CROSS OUT — Navigation directive.
  - Exact span: `source[1202:1233]`.


ADDITIONAL / RECOVERED CLAIMS:

- [x] R17: Nobody gave you an exam in finance when you decided to start a business.
  - Exact span: `source[444:516]`.
  - Coverage: Replaces mixed question/assertion proposal #17.
  - Hedge markers: none.

## fi4e-gold-ep02 (15 proposals)

SOURCE TEXT (1385 chars):

UNDERSTANDING THE FOUNDATION Not long ago, two acquaintances of ours were running their own business. They loved what they were doing. Their company seemed very successful—in fact, it was doubling its sales every year. The financial reports showed that the company was making money. At one point the entrepreneurs enthusiastically showed these reports to another friend, an experienced businessman, who perused them carefully. Box Definitions We want to make finance as easy as possible. Most finance books make us flip back and forth between the page we’re on and the glossary to learn the definition of a word we don’t know. By the time we find it and get back to our page, we’ve lost our train of thought. So here we are going to give you the definitions right where you need them, near the first time we use the word. “I’m very afraid,” he said, “that you will run out of cash in about eighteen months unless you take action now.” Frankly, the entrepreneurs didn’t believe him. They wrote him off as a doomsayer. They knew their business was profitable, and they were certain that their hard work could overcome any obstacle. You can probably see where we’re going with this cautionary tale. Sure enough: in just about eighteen months, they called their friend to admit that they were maxed out on their credit cards and on the home-equity loans they had taken out on their houses.

EXTRACTOR PROPOSALS:

- [ ] #29: ~~UNDERSTANDING THE FOUNDATION Not long ago, two acquaintances of ours were running their own business.~~
  - Decision: CROSS OUT — Heading contaminates assertion; clean span recovered below.
  - Exact span: `source[0:101]`.
- [x] #30: They loved what they were doing.
  - Exact span: `source[102:134]`.
  - Hedge markers: none.
- [x] #31: Their company seemed very successful—in fact, it was doubling its sales every year.
  - Exact span: `source[135:218]`.
  - Hedge markers: none.
- [x] #32: The financial reports showed that the company was making money.
  - Exact span: `source[219:282]`.
  - Hedge markers: none.
- [x] #33: At one point the entrepreneurs enthusiastically showed these reports to another friend, an experienced businessman, who perused them carefully.
  - Exact span: `source[283:426]`.
  - Hedge markers: none.
- [ ] #34: ~~Box Definitions We want to make finance as easy as possible.~~
  - Decision: CROSS OUT — Box heading contaminates assertion; clean span recovered below.
  - Exact span: `source[427:487]`.
- [x] #35: Most finance books make us flip back and forth between the page we’re on and the glossary to learn the definition of a word we don’t know.
  - Exact span: `source[488:626]`.
  - Hedge markers: none.
- [x] #36: By the time we find it and get back to our page, we’ve lost our train of thought.
  - Exact span: `source[627:708]`.
  - Hedge markers: none.
- [x] #37: So here we are going to give you the definitions right where you need them, near the first time we use the word.
  - Exact span: `source[709:821]`.
  - Hedge markers: none.
- [x] #38: “I’m very afraid,” he said, “that you will run out of cash in about eighteen months unless you take action now.”
  - Exact span: `source[822:934]`.
  - Hedge markers: none.
- [x] #39: Frankly, the entrepreneurs didn’t believe him.
  - Exact span: `source[935:981]`.
  - Hedge markers: none.
- [x] #40: They wrote him off as a doomsayer.
  - Exact span: `source[982:1016]`.
  - Hedge markers: none.
- [x] #41: They knew their business was profitable, and they were certain that their hard work could overcome any obstacle.
  - Exact span: `source[1017:1129]`.
  - Hedge markers: `could` at `source[1101:1106]`.
- [ ] #42: ~~You can probably see where we’re going with this cautionary tale.~~
  - Decision: CROSS OUT — Rhetorical address to the reader.
  - Exact span: `source[1130:1195]`.
- [x] #43: Sure enough: in just about eighteen months, they called their friend to admit that they were maxed out on their credit cards and on the home-equity loans they had taken out on their houses.
  - Exact span: `source[1196:1385]`.
  - Hedge markers: none.


ADDITIONAL / RECOVERED CLAIMS:

- [x] R29: Not long ago, two acquaintances of ours were running their own business.
  - Exact span: `source[29:101]`.
  - Coverage: Replaces heading-contaminated proposal #29.
  - Hedge markers: none.

- [x] R34: We want to make finance as easy as possible.
  - Exact span: `source[443:487]`.
  - Coverage: Replaces heading-contaminated proposal #34.
  - Hedge markers: none.

## fi4e-gold-ep03 (12 proposals)

SOURCE TEXT (1275 chars):

They had no more cash and no ability to borrow any more. The business was still booming. But if it was to survive, they would have to sell part of it to outside investors and themselves become minority shareholders. Yet their friend had been able to see the problem coming eighteen months ahead of time, just because he was able to read the financial reports—the foundation of financial intelligence. Some entrepreneurs think they don’t need to bother with formal financial reports. They run the business out of their checkbook. Or maybe they get a bookkeeper to pay the bills and keep the records they need for taxes, but they don’t really study the reports she prepares. That may be fine for one-person shops. But the minute your company begins to grow, as the owners of that ill-fated business discovered, you can no longer tell how it’s doing financially just by looking at the checkbook. You need to see—and to understand—the information contained in the income statement, the balance sheet, and the cash flow statement. If you ever want a loan, moreover, or if you want to attract outside investors, your prospective lenders and shareholders will expect to see all these reports. And they will expect you to answer detailed questions about the data the reports contain.

EXTRACTOR PROPOSALS:

- [x] #44: They had no more cash and no ability to borrow any more.
  - Exact span: `source[0:56]`.
  - Hedge markers: none.
- [x] #45: The business was still booming.
  - Exact span: `source[57:88]`.
  - Hedge markers: none.
- [x] #46: But if it was to survive, they would have to sell part of it to outside investors and themselves become minority shareholders.
  - Exact span: `source[89:215]`.
  - Hedge markers: none.
- [x] #47: Yet their friend had been able to see the problem coming eighteen months ahead of time, just because he was able to read the financial reports—the foundation of financial intelligence.
  - Exact span: `source[216:400]`.
  - Hedge markers: none.
- [x] #48: Some entrepreneurs think they don’t need to bother with formal financial reports.
  - Exact span: `source[401:482]`.
  - Hedge markers: none.
- [x] #49: They run the business out of their checkbook.
  - Exact span: `source[483:528]`.
  - Hedge markers: none.
- [x] #50: Or maybe they get a bookkeeper to pay the bills and keep the records they need for taxes, but they don’t really study the reports she prepares.
  - Exact span: `source[529:672]`.
  - Hedge markers: none.
- [x] #51: That may be fine for one-person shops.
  - Exact span: `source[673:711]`.
  - Hedge markers: `may` at `source[678:681]`.
- [x] #52: But the minute your company begins to grow, as the owners of that ill-fated business discovered, you can no longer tell how it’s doing financially just by looking at the checkbook.
  - Exact span: `source[712:892]`.
  - Hedge markers: none.
- [x] #53: You need to see—and to understand—the information contained in the income statement, the balance sheet, and the cash flow statement.
  - Exact span: `source[893:1025]`.
  - Hedge markers: none.
- [x] #54: If you ever want a loan, moreover, or if you want to attract outside investors, your prospective lenders and shareholders will expect to see all these reports.
  - Exact span: `source[1026:1185]`.
  - Hedge markers: none.
- [x] #55: And they will expect you to answer detailed questions about the data the reports contain.
  - Exact span: `source[1186:1275]`.
  - Hedge markers: none.


ADDITIONAL / RECOVERED CLAIMS:

None; the retained proposals cover the asserted content under this policy.

## fi4e-gold-ep04 (14 proposals)

SOURCE TEXT (1363 chars):

Entrepreneurs who are financially intelligent understand these basics. They can read an income statement, a balance sheet, and a cash flow statement. They know the difference between profitability and a healthy cash flow. (As our story suggests, understanding cash is particularly important to entrepreneurs.) They understand why the balance sheet balances. The numbers neither scare nor mystify them. We’ll consider the three main financial reports in parts 2, 3, and 4 of the book—and we’ll answer questions such as why profit isn’t the same as cash. Income Statement The income statement shows revenues, expenses, and profit for a period of time, such as a month, quarter, or year. It’s also called a profit and loss statement, P&L, statement of earnings, or statement of operations. Big companies sometimes throw the word consolidated in front of those phrases, but it’s still just an income statement. The bottom line of the income statement is net profit, also known as net income or net earnings. We explain the income statement in part 2. UNDERSTANDING THE ART OF FINANCE A second aspect of financial intelligence is understanding what might be called the art of finance. In the preface we referred to it as the finance profession’s little secret, but it isn’t really a secret; it’s a widely acknowledged truth that everyone who has studied finance knows.

EXTRACTOR PROPOSALS:

- [x] #56: Entrepreneurs who are financially intelligent understand these basics.
  - Exact span: `source[0:70]`.
  - Hedge markers: none.
- [x] #57: They can read an income statement, a balance sheet, and a cash flow statement.
  - Exact span: `source[71:149]`.
  - Hedge markers: none.
- [x] #58: They know the difference between profitability and a healthy cash flow.
  - Exact span: `source[150:221]`.
  - Hedge markers: none.
- [x] #59: (As our story suggests, understanding cash is particularly important to entrepreneurs.)
  - Exact span: `source[222:309]`.
  - Hedge markers: none.
- [x] #60: They understand why the balance sheet balances.
  - Exact span: `source[310:357]`.
  - Hedge markers: none.
- [x] #61: The numbers neither scare nor mystify them.
  - Exact span: `source[358:401]`.
  - Hedge markers: none.
- [ ] #62: ~~We’ll consider the three main financial reports in parts 2, 3, and 4 of the book—and we’ll answer questions such as why profit isn’t the same as cash.~~
  - Decision: CROSS OUT — Book navigation; the embedded profit/cash distinction is recovered below.
  - Exact span: `source[402:552]`.
- [ ] #63: ~~Income Statement The income statement shows revenues, expenses, and profit for a period of time, such as a month, quarter, or year.~~
  - Decision: CROSS OUT — Definition heading contaminates assertion; clean span recovered below.
  - Exact span: `source[553:684]`.
- [x] #64: It’s also called a profit and loss statement, P&L, statement of earnings, or statement of operations.
  - Exact span: `source[685:786]`.
  - Hedge markers: none.
- [x] #65: Big companies sometimes throw the word consolidated in front of those phrases, but it’s still just an income statement.
  - Exact span: `source[787:906]`.
  - Hedge markers: none.
- [x] #66: The bottom line of the income statement is net profit, also known as net income or net earnings.
  - Exact span: `source[907:1003]`.
  - Hedge markers: none.
- [ ] #67: ~~We explain the income statement in part 2.~~
  - Decision: CROSS OUT — Book navigation.
  - Exact span: `source[1004:1046]`.
- [ ] #68: ~~UNDERSTANDING THE ART OF FINANCE A second aspect of financial intelligence is understanding what might be called the art of finance.~~
  - Decision: CROSS OUT — Section heading contaminates assertion; clean span recovered below.
  - Exact span: `source[1047:1179]`.
- [x] #69: In the preface we referred to it as the finance profession’s little secret, but it isn’t really a secret; it’s a widely acknowledged truth that everyone who has studied finance knows.
  - Exact span: `source[1180:1363]`.
  - Hedge markers: none.


ADDITIONAL / RECOVERED CLAIMS:

- [x] M62: profit isn’t the same as cash
  - Exact span: `source[522:551]`.
  - Coverage: Embedded assertion in navigation proposal #62.
  - Hedge markers: none.

- [x] R63: The income statement shows revenues, expenses, and profit for a period of time, such as a month, quarter, or year.
  - Exact span: `source[570:684]`.
  - Coverage: Replaces heading-contaminated proposal #63.
  - Hedge markers: none.

- [x] R68: A second aspect of financial intelligence is understanding what might be called the art of finance.
  - Exact span: `source[1080:1179]`.
  - Coverage: Replaces heading-contaminated proposal #68.
  - Hedge markers: `might` at `source[1144:1149]`.

## fi4e-gold-ep05 (10 proposals)

SOURCE TEXT (1281 chars):

Trouble is, the rest of us tend to forget it. We think that if a number shows up on a financial statement or on your accountants’ reports, it must accurately represent reality. Of course, that can’t always be true, if only because bookkeepers and accountants can’t know everything. They can’t know exactly what everyone in the company does every day, so they don’t know exactly how to allocate costs. They can’t know exactly how long a piece of equipment will last, so they don’t know how much of its original cost to record in any given year. The art of accounting and finance is the art of using limited data to come as close as possible to an accurate description of how well a company is performing. Accounting and finance are not reality; they are a reflection of reality, and the accuracy of that reflection depends on the ability of bookkeepers, accountants, and finance professionals to make reasonable assumptions and to calculate reasonable estimates. It’s a tough job. Sometimes the accountants and finance folks have to quantify what can’t easily be quantified. Sometimes they have to make difficult judgments about how to categorize a given item. None of these complications necessarily arises because they are trying to cook the books or because they are incompetent.

EXTRACTOR PROPOSALS:

- [x] #70: Trouble is, the rest of us tend to forget it.
  - Exact span: `source[0:45]`.
  - Hedge markers: none.
- [x] #71: We think that if a number shows up on a financial statement or on your accountants’ reports, it must accurately represent reality.
  - Exact span: `source[46:176]`.
  - Hedge markers: none.
- [x] #72: Of course, that can’t always be true, if only because bookkeepers and accountants can’t know everything.
  - Exact span: `source[177:281]`.
  - Hedge markers: none.
- [x] #73: They can’t know exactly what everyone in the company does every day, so they don’t know exactly how to allocate costs.
  - Exact span: `source[282:400]`.
  - Hedge markers: none.
- [x] #74: They can’t know exactly how long a piece of equipment will last, so they don’t know how much of its original cost to record in any given year.
  - Exact span: `source[401:543]`.
  - Hedge markers: none.
- [x] #75: The art of accounting and finance is the art of using limited data to come as close as possible to an accurate description of how well a company is performing.
  - Exact span: `source[544:703]`.
  - Hedge markers: none.
- [x] #76: Accounting and finance are not reality; they are a reflection of reality, and the accuracy of that reflection depends on the ability of bookkeepers, accountants, and finance professionals to make reasonable assumptions and to calculate reasonable estimates.
  - Exact span: `source[704:961]`.
  - Hedge markers: `estimates` at `source[951:960]`.
- [x] #77: It’s a tough job. Sometimes the accountants and finance folks have to quantify what can’t easily be quantified.
  - Exact span: `source[962:1073]`.
  - Hedge markers: none.
- [x] #78: Sometimes they have to make difficult judgments about how to categorize a given item.
  - Exact span: `source[1074:1159]`.
  - Hedge markers: none.
- [x] #79: None of these complications necessarily arises because they are trying to cook the books or because they are incompetent.
  - Exact span: `source[1160:1281]`.
  - Hedge markers: none.


ADDITIONAL / RECOVERED CLAIMS:

None; the retained proposals cover the asserted content under this policy.

## fi4e-gold-ep06 (12 proposals)

SOURCE TEXT (1337 chars):

The complications arise because they must make educated guesses relating to the numbers side of the business all day long. Balance Sheet The balance sheet reflects the assets, liabilities, and owners’ equity at a point in time. In other words, it shows, on a specific day, what the company owned, what it owed, and how much it was worth. The balance sheet is called such because it balances—assets always must equal liabilities plus owners’ equity. A financially savvy entrepreneur knows that all the financial statements ultimately flow to the balance sheet. Part 3 takes up the balance sheet. The result of these assumptions and estimates is, typically, a bias in the numbers. Please don’t get the idea that by using the word bias we are impugning anybody’s integrity. (Some of our best friends are accountants—no, really—and one of us, Joe, actually carries the title CFO on his business card.) Where financial results are concerned, bias means only that the numbers might be skewed in one direction or another. It means only that bookkeepers, accountants, and finance professionals have used certain assumptions and estimates rather than others when they put their reports together. Enabling you to understand this bias, to correct for it where necessary, and even to use it to your company’s advantage is one objective of this book.

EXTRACTOR PROPOSALS:

- [x] #80: The complications arise because they must make educated guesses relating to the numbers side of the business all day long.
  - Exact span: `source[0:122]`.
  - Hedge markers: none.
- [ ] #81: ~~Balance Sheet The balance sheet reflects the assets, liabilities, and owners’ equity at a point in time.~~
  - Decision: CROSS OUT — Definition heading contaminates assertion; clean span recovered below.
  - Exact span: `source[123:227]`.
- [x] #82: In other words, it shows, on a specific day, what the company owned, what it owed, and how much it was worth.
  - Exact span: `source[228:337]`.
  - Hedge markers: none.
- [x] #83: The balance sheet is called such because it balances—assets always must equal liabilities plus owners’ equity.
  - Exact span: `source[338:448]`.
  - Hedge markers: none.
- [x] #84: A financially savvy entrepreneur knows that all the financial statements ultimately flow to the balance sheet.
  - Exact span: `source[449:559]`.
  - Hedge markers: none.
- [ ] #85: ~~Part 3 takes up the balance sheet.~~
  - Decision: CROSS OUT — Book navigation.
  - Exact span: `source[560:594]`.
- [x] #86: The result of these assumptions and estimates is, typically, a bias in the numbers.
  - Exact span: `source[595:678]`.
  - Hedge markers: `estimates` at `source[631:640]`.
- [ ] #87: ~~Please don’t get the idea that by using the word bias we are impugning anybody’s integrity.~~
  - Decision: CROSS OUT — Directive about interpreting the authors’ intent.
  - Exact span: `source[679:770]`.
- [x] #88: (Some of our best friends are accountants—no, really—and one of us, Joe, actually carries the title CFO on his business card.)
  - Exact span: `source[771:897]`.
  - Hedge markers: none.
- [x] #89: Where financial results are concerned, bias means only that the numbers might be skewed in one direction or another.
  - Exact span: `source[898:1014]`.
  - Hedge markers: `might` at `source[970:975]`.
- [x] #90: It means only that bookkeepers, accountants, and finance professionals have used certain assumptions and estimates rather than others when they put their reports together.
  - Exact span: `source[1015:1186]`.
  - Hedge markers: `estimates` at `source[1120:1129]`.
- [x] #91: Enabling you to understand this bias, to correct for it where necessary, and even to use it to your company’s advantage is one objective of this book.
  - Exact span: `source[1187:1337]`.
  - Hedge markers: none.


ADDITIONAL / RECOVERED CLAIMS:

- [x] R81: The balance sheet reflects the assets, liabilities, and owners’ equity at a point in time.
  - Exact span: `source[137:227]`.
  - Coverage: Replaces heading-contaminated proposal #81.
  - Hedge markers: none.

## fi4e-gold-ep07 (9 proposals)

SOURCE TEXT (1239 chars):

So financially intelligent entrepreneurs are able to identify where the artful aspects of finance have been applied to the numbers, and they know how applying them differently might lead to different conclusions. They are prepared, when appropriate, to question and challenge the numbers they get from their accountants or finance folks. In the following chapter we’ll show you some specific examples of the art of finance, but it’s a lesson you’ll want to bear in mind throughout the book. UNDERSTANDING FINANCIAL ANALYSIS Once you can read the financials, and once you have an appreciation of the art of finance, you can use the information to analyze the numbers in greater depth and to make decisions based on what you learn. For example, did you know the following? A couple of simple ratios derived from the balance sheet will tell you at a glance whether you’re going to be able to pay your bills during the coming year. If you can’t pay your bills, you may decide to apply for a loan. These are the same ratios bankers will use to make an initial judgment about whether they should consider your company creditworthy. Profitability ratios—derived from the income statement—help you understand how much money your company is making.

EXTRACTOR PROPOSALS:

- [x] #92: So financially intelligent entrepreneurs are able to identify where the artful aspects of finance have been applied to the numbers, and they know how applying them differently might lead to different conclusions.
  - Exact span: `source[0:212]`.
  - Hedge markers: `might` at `source[176:181]`.
- [x] #93: They are prepared, when appropriate, to question and challenge the numbers they get from their accountants or finance folks.
  - Exact span: `source[213:337]`.
  - Hedge markers: none.
- [ ] #94: ~~In the following chapter we’ll show you some specific examples of the art of finance, but it’s a lesson you’ll want to bear in mind throughout the book.~~
  - Decision: CROSS OUT — Book navigation and reading advice.
  - Exact span: `source[338:490]`.
- [ ] #95: ~~UNDERSTANDING FINANCIAL ANALYSIS Once you can read the financials, and once you have an appreciation of the art of finance, you can use the information to analyze the numbers in greater depth and to make decisions based on what you learn.~~
  - Decision: CROSS OUT — Section heading contaminates assertion; clean span recovered below.
  - Exact span: `source[491:729]`.
- [ ] #96: ~~For example, did you know the following?~~
  - Decision: CROSS OUT — Rhetorical introductory question.
  - Exact span: `source[730:770]`.
- [x] #97: A couple of simple ratios derived from the balance sheet will tell you at a glance whether you’re going to be able to pay your bills during the coming year.
  - Exact span: `source[771:927]`.
  - Hedge markers: none.
- [x] #98: If you can’t pay your bills, you may decide to apply for a loan.
  - Exact span: `source[928:992]`.
  - Hedge markers: `may` at `source[961:964]`.
- [x] #99: These are the same ratios bankers will use to make an initial judgment about whether they should consider your company creditworthy.
  - Exact span: `source[993:1125]`.
  - Hedge markers: none.
- [x] #100: Profitability ratios—derived from the income statement—help you understand how much money your company is making.
  - Exact span: `source[1126:1239]`.
  - Hedge markers: none.

ADDITIONAL / RECOVERED CLAIMS:

- [x] R95: Once you can read the financials, and once you have an appreciation of the art of finance, you can use the information to analyze the numbers in greater depth and to make decisions based on what you learn.
  - Exact span: `source[524:729]`.
  - Coverage: Replaces heading-contaminated proposal #95.
  - Hedge markers: none.

## Draft completeness check

All 101 extractor proposals adjudicated: 76 kept and 25 crossed out.
Added 7 cleaned replacements (6 heading repairs and 1 question/assertion
repair) and 3 embedded-content recoveries, yielding 86 retained claim spans.
All 10 additions come from text already covered by extractor proposals; none
comes from a source passage omitted by the extractor. The 3 embedded recoveries
are claims not separately proposed, rather than previously uncovered text.
Every retained claim has a verified exact source span and an explicit hedge
inventory.
Human review and sign-off: COMPLETE (2026-11-04). This copy is the
provenance record for evals/fixtures/gold-fi4e-v1.json.
