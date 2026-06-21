module DebuggerTest where


fact :: Int -> Int
fact 0 = 1
fact n = n * fact (n - 1)

sumSquares :: [Int] -> Int
sumSquares xs =
  let ys = map (\x -> x * x) xs
      total = sum ys
  in total

classify :: Int -> String
classify n =
  if n < 0
    then "negative"
    else if n == 0
      then "zero"
      else "positive"

lazyHead :: Int
lazyHead =
  let xs = [1 ..]
      ys = map (* 10) xs
  in head ys
