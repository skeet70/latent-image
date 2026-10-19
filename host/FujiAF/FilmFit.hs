-- Scene rendering through a sensor model and the film stage, and the integer
-- fit of a colour matrix (Q10 rows summing to 1024) and chroma 2x2 (Q10) that
-- makes one pipeline render a patch set as a reference rendering does.
module FujiAF.FilmFit
  ( Exposure (..)
  , sceneInput
  , renderLab
  , calibrateExposure
  , ColourSetting (..)
  , settingOf
  , applySetting
  , FitResult (..)
  , Space
  , fullSpace
  , matrixSpace
  , ridgeSpace
  , fitColour
  , fitIn
  , Stats (..)
  , stats
  , deltas
  ) where

import Data.List (minimumBy, sort, sortOn)
import Data.Ord (comparing)

import FujiAF.Colour
import FujiAF.FilmStage

-- | Scale from white-balanced camera RGB (white = 1) to 14-bit stage input.
newtype Exposure = Exposure Double deriving (Show)

sceneInput :: CameraModel -> Exposure -> Xyz -> Rgb14
sceneInput cam (Exposure k) xyz = Rgb14 (q r) (q g) (q b)
 where
  Rgb r g b = whiteBalanced cam xyz
  q v = max 0 (min 16383 (round (k * v)))

renderLab :: Stage -> Rgb14 -> Lab
renderLab st w = let YCbCr y cb cr = develop st w in ycbcrToLab y cb cr

-- | The smallest exposure that renders an 18 % D65 grey at 8-bit luma 118
-- (sRGB L* 50) under the given stage.
calibrateExposure :: Stage -> Exposure
calibrateExposure st = Exposure (go 1 200000 (40 :: Int))
 where
  lumaAt k = let v = max 0 (min 16383 (round (0.18 * k))) in ycY (develop st (Rgb14 v v v))
  go _ hi 0 = hi
  go lo hi n = let mid = (lo + hi) / 2 in if lumaAt mid >= 118 then go lo mid (n - 1) else go mid hi (n - 1)

-- | settingOf takes the matrix off-diagonals (R: g, b; G: r, b; B: r, g)
-- and the chroma 2x2 (Cr from R-Y, Cr from B-Y, Cb from R-Y, Cb from B-Y).
data ColourSetting = ColourSetting {settingMatrix :: ColourMatrix, settingChroma :: Chroma} deriving (Eq, Show)

settingOf :: [Int] -> Maybe ColourSetting
settingOf [rg, rb, gr, gb, br, bg, c0, c1, c2, c3] =
  either (const Nothing) Just $
    ColourSetting
      <$> colourMatrix [[1024 - rg - rb, rg, rb], [gr, 1024 - gr - gb, gb], [br, bg, 1024 - br - bg]]
      <*> chroma c0 c1 c2 c3
settingOf _ = Nothing

coordinates :: ColourSetting -> [Int]
coordinates (ColourSetting m (Chroma c0 c1 c2 c3)) = case matrixRows m of
  [[_, rg, rb], [gr, _, gb], [br, bg, _]] -> [rg, rb, gr, gb, br, bg, c0, c1, c2, c3]
  _ -> error "matrix has three rows of three"

applySetting :: ColourSetting -> Stage -> Stage
applySetting (ColourSetting m c) = withColour m c

data Stats = Stats {statMean :: Double, statP95 :: Double, statMax :: Double} deriving (Show)

-- | Mean, 95th percentile (nearest rank) and maximum.
stats :: [Double] -> Stats
stats [] = Stats 0 0 0
stats xs = Stats (sum xs / fromIntegral n) (sorted !! (max 1 (ceiling (0.95 * fromIntegral n :: Double)) - 1)) (last sorted)
 where
  sorted = sort xs
  n = length xs

deltas :: Stage -> [Rgb14] -> [Lab] -> [Double]
deltas st inputs refs = zipWith (\w ref -> ciede2000 ref (renderLab st w)) inputs refs

data FitResult = FitResult {fitSetting :: ColourSetting, fitObjective :: Double, fitEvaluations :: Int} deriving (Show)

-- | The integers a fit varies, the setting they make, and ridge residuals
-- added to the objective.
data Space = Space
  { spaceSetting :: [Int] -> Maybe ColourSetting
  , spaceCoordinates :: ColourSetting -> [Int]
  , spacePenalty :: [Int] -> [Double]
  }

chromaList :: Chroma -> [Int]
chromaList (Chroma a b c d) = [a, b, c, d]

fullSpace :: Space
fullSpace = Space settingOf coordinates (const [])

-- | The six matrix off-diagonals; chroma held at the given value.
matrixSpace :: Chroma -> Space
matrixSpace c = Space (\xs -> settingOf (xs ++ chromaList c)) (take 6 . coordinates) (const [])

-- | All ten integers, each chroma term pulled toward the prior with residual
-- sqrt w * (c - prior) / 100.
ridgeSpace :: Double -> Chroma -> Space
ridgeSpace w c = Space settingOf coordinates (\xs -> [sqrt w * fromIntegral (x - p) / 100 | (x, p) <- zip (drop 6 xs) (chromaList c)])

fitColour :: Stage -> [Rgb14] -> [Lab] -> [ColourSetting] -> FitResult
fitColour = fitIn fullSpace

-- | Minimises the mean squared CIEDE2000 between the stage with a candidate
-- setting and the target Labs, plus the space's squared ridge residuals. Each
-- start is first moved by Levenberg-Marquardt on the L*a*b* differences; from
-- there, rounds of Nelder-Mead on rounded coordinates followed by integer
-- coordinate search (steps 64 down to 1) repeat until a round no longer
-- improves, at most eight rounds.
fitIn :: Space -> Stage -> [Rgb14] -> [Lab] -> [ColourSetting] -> FitResult
fitIn space st inputs refs starts =
  minimumOn fitObjective [rounds (8 :: Int) (FitResult s' (scoreI (coords s')) 1) | s <- starts, s' <- [s, levenberg s]]
 where
  coords = spaceCoordinates space
  n = fromIntegral (length refs) :: Double
  objective xs cs = let ds = deltas (applySetting cs st) inputs refs in sum (map (\d -> d * d) ds) / n + sum (map (\p -> p * p) (spacePenalty space xs))
  scoreI xs = maybe (1 / 0) (objective xs) (spaceSetting space xs)
  score = scoreI . map round
  residuals xs = case spaceSetting space (map round xs) of
    Nothing -> Nothing
    Just cs ->
      Just
        ( concat [[(l1 - l0) / sqrt n, (a1 - a0) / sqrt n, (b1 - b0) / sqrt n] | (Lab l0 a0 b0, w) <- zip refs inputs, let Lab l1 a1 b1 = renderLab (applySetting cs st) w]
            ++ spacePenalty space (map round xs)
        )
  levenberg s = maybe s id (spaceSetting space (map round (marquardt residuals (map fromIntegral (coords s)))))
  rounds 0 r = r
  rounds k r =
    let (best, evals) = nelderMead score (map fromIntegral (coords (fitSetting r))) 64 4000
        xs = map round best
        next = search xs (scoreI xs) [64, 32, 16, 8, 4, 2, 1] (fitEvaluations r + evals)
     in if fitObjective next < fitObjective r - 1e-9 then rounds (k - 1) next else r {fitEvaluations = fitEvaluations next}
  search xs v [] evals = FitResult (maybe (error "fit left the field range") id (spaceSetting space xs)) v evals
  search xs v (step : rest) evals =
    case [(ys, sv) | i <- [0 .. length xs - 1], d <- [step, -step], let ys = bump i d xs, let sv = scoreI ys, sv < v] of
      ((ys, sv) : _) -> search ys sv (step : rest) (evals + 2 * length xs)
      [] -> search xs v rest (evals + 2 * length xs)
  bump i d xs = [if j == i then x + d else x | (j, x) <- zip [0 :: Int ..] xs]

minimumOn :: Ord b => (a -> b) -> [a] -> a
minimumOn f = minimumBy (comparing f)

-- | Levenberg-Marquardt on a residual vector with central differences of
-- step 32; at most 60 iterations.
marquardt :: ([Double] -> Maybe [Double]) -> [Double] -> [Double]
marquardt res x0 = go x0 1e-3 (60 :: Int)
 where
  sumSq = maybe (1 / 0) (sum . map (\v -> v * v))
  go x _ 0 = x
  go x lambda n = case res x of
    Nothing -> x
    Just r ->
      let k = length x
          cols = [zipWith (\p m -> (p - m) / 64) (maybe r id (res (bump i 32 x))) (maybe r id (res (bump i (-32) x))) | i <- [0 .. k - 1]]
          jtj = [[sum (zipWith (*) ci cj) | cj <- cols] | ci <- cols]
          jtr = [sum (zipWith (*) ci r) | ci <- cols]
          step l = solve [[if i == j then v * (1 + l) + 1e-9 else v | (j, v) <- zip [0 :: Int ..] row] | (i, row) <- zip [0 ..] jtj] (map negate jtr)
          current = sumSq (Just r)
          try l tries
            | tries > (8 :: Int) = Nothing
            | otherwise =
                let x' = zipWith (+) x (step l)
                 in if sumSq (res x') < current then Just (x', l / 3) else try (l * 4) (tries + 1)
       in case try lambda 0 of
            Just (x', l') | maximum (map abs (zipWith (-) x' x)) >= 0.5 -> go x' l' (n - 1)
            Just (x', _) -> x'
            Nothing -> x
  bump i d x = [if j == i then v + d else v | (j, v) <- zip [0 :: Int ..] x]

-- | Gaussian elimination with partial pivoting.
solve :: [[Double]] -> [Double] -> [Double]
solve a b = back (forward (zipWith (\row v -> row ++ [v]) a b))
 where
  forward [] = []
  forward rows =
    let pivot = maximumOn (abs . headOr0) rows
        others = deleteFirst pivot rows
        p = headOr0 pivot
        eliminate row = let f = headOr0 row / p in drop 1 (zipWith (\u v -> u - f * v) row pivot)
     in pivot : forward (map eliminate others)
  back [] = []
  back (row : rest) =
    let xs = back rest
        coeffs = drop 1 (init row)
        rhs = last row
     in (rhs - sum (zipWith (*) coeffs xs)) / headOr0 row : xs
  headOr0 (v : _) = v
  headOr0 [] = 0
  maximumOn f = foldr1 (\u v -> if f u >= f v then u else v)
  deleteFirst x (y : ys) = if x == y then ys else y : deleteFirst x ys
  deleteFirst _ [] = []

-- | Nelder-Mead with standard coefficients; returns the best vertex and the
-- evaluation count.
nelderMead :: ([Double] -> Double) -> [Double] -> Double -> Int -> ([Double], Int)
nelderMead f x0 size budget = loop (sortOn snd [(v, f v) | v <- initial]) (length initial)
 where
  n = length x0
  initial = x0 : [[if j == i then x + size else x | (j, x) <- zip [0 ..] x0] | i <- [0 .. n - 1]]
  add = zipWith (+)
  sub = zipWith (-)
  scale k = map (* k)
  loop [] evals = (x0, evals)
  loop vs@(best : others) evals
    | evals >= budget || snd (last vs) - snd best < 1e-9 = (fst best, evals)
    | otherwise =
        let worst = last vs
            second = vs !! (n - 1)
            centroid = scale (1 / fromIntegral n) (foldl' add (replicate n 0) (map fst (init vs)))
            xr = add centroid (sub centroid (fst worst))
            fr = f xr
         in if fr < snd best
              then
                let xe = add centroid (scale 2 (sub xr centroid))
                    fe = f xe
                 in loop (sortOn snd (init vs ++ [if fe < fr then (xe, fe) else (xr, fr)])) (evals + 2)
              else
                if fr < snd second
                  then loop (sortOn snd (init vs ++ [(xr, fr)])) (evals + 1)
                  else
                    let xc = add centroid (scale 0.5 (sub (fst worst) centroid))
                        fc = f xc
                     in if fc < snd worst
                          then loop (sortOn snd (init vs ++ [(xc, fc)])) (evals + 2)
                          else
                            let shrunk = [let x = add (fst best) (scale 0.5 (sub (fst v) (fst best))) in (x, f x) | v <- others]
                             in loop (sortOn snd (best : shrunk)) (evals + 1 + n)
